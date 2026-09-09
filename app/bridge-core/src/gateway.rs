//! 对外网关:ChatGPT 唯一可见的入口(OAuth 2.1 模式)。
//!
//! - 只绑定 127.0.0.1,公网唯一入口是隧道
//! - 鉴权:OAuth 2.1(DCR + 授权码 + PKCE + refresh 轮换),见 oauth.rs
//! - 401 携带 WWW-Authenticate: Bearer resource_metadata=...(RFC 9728)
//! - OpenAI Tunnel 模式无公网入口,可切免鉴权
//! - 性能:tools/list 回缓存;initialize 本地应答;输出超限截断

use anyhow::Result;
use axum::{
    extract::{Form, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use crate::logbus::{LogBus, PairBus};
use crate::mcp::Upstream;
use crate::oauth::OAuthServer;

/// 工具输出截断阈值(字节)
const MAX_OUTPUT_BYTES: usize = 8 * 1024;

#[derive(Clone)]
struct GwState {
    upstream: Arc<Upstream>,
    oauth: Arc<OAuthServer>,
    log: LogBus,
    pair: PairBus,
    allow_noauth: bool,
}

pub struct GatewayHandle {
    pub port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl GatewayHandle {
    pub fn stop(self) {
        let _ = self.shutdown.send(());
    }
}

fn public_base(headers: &HeaderMap) -> String {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!("https://{}", host)
}

/// 401 + RFC 9728 WWW-Authenticate 头,ChatGPT 据此发现 OAuth 元数据
fn unauthorized(base: &str) -> Response {
    let mut r = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"jsonrpc":"2.0","error":{"code":-32001,"message":"unauthorized"},"id":null})),
    )
        .into_response();
    r.headers_mut().insert(
        "www-authenticate",
        HeaderValue::from_str(&format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
            base
        ))
        .unwrap(),
    );
    r
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// 校验请求方 token;accepted aud 为 origin 与 origin/mcp 两种写法
fn authorized(s: &GwState, headers: &HeaderMap) -> bool {
    if s.allow_noauth {
        return true;
    }
    let Some(token) = bearer_token(headers) else {
        return false;
    };
    let base = public_base(headers);
    let accepted = vec![base.clone(), format!("{}/mcp", base)];
    s.oauth.verify(token, &accepted)
}

fn truncate_result(mut v: Value) -> Value {
    if let Some(arr) = v.pointer_mut("/result/content").and_then(Value::as_array_mut) {
        for item in arr.iter_mut() {
            let new_text = item.get("text").and_then(Value::as_str).and_then(|text| {
                if text.len() > MAX_OUTPUT_BYTES {
                    let mut cut: String = text.chars().take(MAX_OUTPUT_BYTES).collect();
                    cut.push_str(&format!(
                        "\n\n… [MCP Bridge] 输出过长已截断(原 {} KB),请缩小范围或分页",
                        text.len() / 1024
                    ));
                    Some(cut)
                } else {
                    None
                }
            });
            if let Some(t) = new_text {
                item["text"] = Value::String(t);
            }
        }
    }
    v
}

async fn handle_rpc(state: &GwState, body: Value) -> (StatusCode, Json<Value>) {
    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let t0 = Instant::now();

    match method {
        "initialize" => (
            StatusCode::OK,
            Json(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "MCP Bridge (JetBrains)", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
        ),
        m if m.starts_with("notifications/") => (StatusCode::ACCEPTED, Json(Value::Null)),
        "ping" => (StatusCode::OK, Json(json!({"jsonrpc":"2.0","id":id,"result":{}}))),
        "tools/list" => match state.upstream.tools(false).await {
            Ok(tools) => (
                StatusCode::OK,
                Json(json!({"jsonrpc":"2.0","id":id,"result":{"tools": tools.as_ref()}})),
            ),
            Err(e) => (
                StatusCode::OK,
                Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":e.to_string()}})),
            ),
        },
        "tools/call" => {
            let name = body
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = body.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            let out = state.upstream.call_tool(&name, args).await;
            let elapsed = t0.elapsed().as_millis();
            match out {
                Ok(v) => {
                    let v = truncate_result(v);
                    let result = v.get("result").cloned().unwrap_or(Value::Null);
                    let size = result.to_string().len();
                    state.log.emit("INFO", "网关", format!("tools/call {} · {} ms · {} B", name, elapsed, size));
                    (StatusCode::OK, Json(json!({"jsonrpc":"2.0","id":id,"result": result})))
                }
                Err(e) => {
                    state.log.emit("ERR", "网关", format!("tools/call {} 失败: {}", name, e));
                    (
                        StatusCode::OK,
                        Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":e.to_string()}})),
                    )
                }
            }
        }
        _ => (
            StatusCode::OK,
            Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("method not found: {}",method)}})),
        ),
    }
}

async fn mcp_post(State(s): State<GwState>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if !authorized(&s, &headers) {
        s.log.emit("WARN", "网关", "拒绝未授权请求(401,OAuth 挑战已下发)");
        return unauthorized(&public_base(&headers));
    }
    handle_rpc(&s, body).await.into_response()
}

/* ── OAuth 端点 ── */

/// RFC 9728 Protected Resource Metadata
async fn protected_resource(headers: HeaderMap) -> Json<Value> {
    let base = public_base(&headers);
    Json(json!({
        "resource": base,
        "authorization_servers": [base],
        "scopes_supported": ["mcp"]
    }))
}

/// RFC 8414 Authorization Server Metadata(必须含 S256,否则 ChatGPT 不支持)
async fn as_metadata(headers: HeaderMap) -> Json<Value> {
    let base = public_base(&headers);
    Json(json!({
        "issuer": base,
        "authorization_response_iss_parameter_supported": true,
        "authorization_endpoint": format!("{}/oauth/authorize", base),
        "token_endpoint": format!("{}/oauth/token", base),
        "registration_endpoint": format!("{}/oauth/register", base),
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "response_types_supported": ["code"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["mcp"]
    }))
}

/// RFC 7591 动态客户端注册
async fn register(State(s): State<GwState>, Json(body): Json<Value>) -> impl IntoResponse {
    let redirect_uris = body
        .get("redirect_uris")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect::<Vec<_>>())
        .unwrap_or_default();
    if redirect_uris.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_client_metadata"}))).into_response();
    }
    let resp = s.oauth.register(redirect_uris);
    s.log.emit("INFO", "网关", "新 OAuth 客户端注册(DCR)");
    (StatusCode::CREATED, Json(resp)).into_response()
}

/// 授权页(GET 显示配对码输入页)
async fn authorize_get(State(s): State<GwState>, Query(q): Query<HashMap<String, String>>, _headers: HeaderMap) -> Response {
    if let Err(e) = s.oauth.check_authorize(&q) {
        s.log.emit("WARN", "网关", format!("授权请求被拒绝: {}", e));
        return Html(render_error(&e)).into_response();
    }
    // 生成配对码并通知桌面弹窗
    let code = s.oauth.create_pending(q.clone());
    s.pair.emit(code.clone());
    s.log.emit("INFO", "网关", format!("配对码已生成({}…),等待网页输码", &code[..5]));
    Html(render_consent(&q, None)).into_response()
}

async fn authorize_post(State(s): State<GwState>, headers: HeaderMap, Form(form): Form<HashMap<String, String>>) -> Response {
    let base = public_base(&headers);
    let code = form.get("pairing_code").cloned().unwrap_or_default();

    if form.get("decision").map(String::as_str) == Some("deny") {
        s.oauth.drop_pending(&code);
        s.log.emit("WARN", "网关", "用户拒绝了授权");
        return Redirect::to(&s.oauth.deny(&form, &base)).into_response();
    }

    // 仅网页输码:配对码有效才放行
    match s.oauth.take_pending(&code) {
        Some(p) => {
            // 完整性:表单与登记时的授权请求必须一致
            let same = form.get("client_id") == p.query.get("client_id") && form.get("state") == p.query.get("state");
            if !same {
                s.log.emit("WARN", "网关", "配对请求与原始授权参数不一致");
                return Html(render_error("授权参数不一致,请重新发起")).into_response();
            }
            s.log.emit("OK", "网关", "配对码验证通过,签发授权码");
            Redirect::to(&s.oauth.approve(&p.query, &base)).into_response()
        }
        None => {
            s.log.emit("WARN", "网关", "配对码错误或已过期");
            Html(render_consent(&form, Some("授权码错误或已过期,请核对软件弹窗中的最新配对码"))).into_response()
        }
    }
}

async fn token(State(s): State<GwState>, Form(form): Form<HashMap<String, String>>) -> Response {
    match s.oauth.token(&form) {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err((code, v)) => {
            s.log.emit("WARN", "网关", format!("token 签发失败: {}", v));
            (StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST), Json(v)).into_response()
        }
    }
}

async fn healthz() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// 启动网关
pub async fn start(
    upstream: Arc<Upstream>,
    oauth: Arc<OAuthServer>,
    log: LogBus,
    pair: PairBus,
    port: u16,
    allow_noauth: bool,
) -> Result<GatewayHandle> {
    let state = GwState {
        upstream,
        oauth,
        log: log.clone(),
        pair,
        allow_noauth,
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/mcp", post(mcp_post))
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(as_metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize_get).post(authorize_post))
        .route("/oauth/token", post(token))
        .with_state(state);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    log.emit("OK", "网关", format!("鉴权网关已启动(OAuth 2.1)→ http://127.0.0.1:{}/mcp", port));
    Ok(GatewayHandle { port, shutdown: tx })
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn hidden_inputs(q: &HashMap<String, String>) -> String {
    q.iter()
        .map(|(k, v)| format!("<input type='hidden' name='{}' value='{}'>", html_escape(k), html_escape(v)))
        .collect()
}

/// 授权配对页(WebGL 液体玻璃折射背景 + 玻璃拟态卡 + 三点连线,零外部依赖)
const AURORA_JS: &str = include_str!("aurora_web.js");

fn render_consent(q: &HashMap<String, String>, error: Option<&str>) -> String {
    let hidden = hidden_inputs(q);
    let err_html = error
        .map(|e| format!(r#"<div class="err">{}</div>"#, html_escape(e)))
        .unwrap_or_default();
    format!(
        r##"<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>MCP Bridge · 授权</title>
<style>
*{{margin:0;box-sizing:border-box}}
body{{min-height:100vh;overflow:hidden;font-family:-apple-system,"Helvetica Neue","PingFang SC",sans-serif;color:#0b0b10}}
#gl{{position:fixed;inset:0;width:100%;height:100%}}
.noise{{position:fixed;inset:0;background-image:radial-gradient(rgba(20,20,40,.13) 1px,transparent 1px);background-size:22px 22px;pointer-events:none}}
/* ── 品牌 ── */
.brand{{position:fixed;top:22px;left:26px;font:600 12px/1 ui-monospace,Menlo,monospace;letter-spacing:.22em;color:rgba(20,20,40,.55);z-index:3}}
.brand b{{color:#0b0b10}}
/* ── 玻璃卡 ── */
.wrap{{position:relative;z-index:2;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px}}
.card{{width:470px;padding:36px 36px 24px;border-radius:22px;
background:rgba(255,255,255,.52);backdrop-filter:blur(26px) saturate(1.5);-webkit-backdrop-filter:blur(26px) saturate(1.5);
border:1px solid rgba(255,255,255,.75);box-shadow:0 24px 70px rgba(40,50,90,.16),inset 0 1px 0 rgba(255,255,255,.9);
animation:rise .55s cubic-bezier(.2,.9,.25,1.1) both}}
@keyframes rise{{from{{opacity:0;transform:translateY(16px) scale(.97)}}to{{opacity:1;transform:none}}}}
.tag{{font:600 11px/1 ui-monospace,Menlo,monospace;letter-spacing:.18em;color:#7a7f96;margin-bottom:14px}}
h1{{font-size:23px;font-weight:700;letter-spacing:-.3px;margin-bottom:8px}}
.sub{{font-size:13px;color:#5b6072;line-height:1.7;margin-bottom:20px}}
/* ── 三点连线 ── */
.link{{display:flex;align-items:center;justify-content:space-between;margin-bottom:22px;padding:0 4px}}
.node{{display:flex;flex-direction:column;align-items:center;gap:6px;font:500 10.5px/1 ui-monospace,Menlo,monospace;color:#7a7f96}}
.node .ic{{width:38px;height:38px;border-radius:12px;background:rgba(255,255,255,.85);border:1px solid rgba(255,255,255,.9);box-shadow:0 4px 14px rgba(40,50,90,.12);display:flex;align-items:center;justify-content:center;font-size:17px}}
.wire{{flex:1;height:2px;margin:0 8px 16px;background:repeating-linear-gradient(90deg,#b8bdd4 0 6px,transparent 6px 12px);position:relative;overflow:visible}}
.wire::after{{content:"";position:absolute;top:-3px;left:0;width:8px;height:8px;border-radius:50%;background:#0b0b10;animation:pulse 2.2s linear infinite}}
@keyframes pulse{{to{{left:calc(100% - 8px)}}}}
/* ── 输入 ── */
input.code{{width:100%;height:66px;border:1.5px solid rgba(20,20,50,.16);border-radius:14px;background:rgba(255,255,255,.65);
font:600 27px/1 ui-monospace,Menlo,monospace;letter-spacing:.16em;text-align:center;color:#0b0b10;outline:none;transition:.18s}}
input.code:focus{{border-color:#0b0b10;background:#fff;box-shadow:0 0 0 4px rgba(11,11,16,.07)}}
input.code::placeholder{{color:#b6bacb;letter-spacing:.14em}}
.err{{margin-top:10px;font-size:12px;color:#D70022;line-height:1.5;animation:rise .3s both}}
.row{{display:flex;gap:10px;margin-top:22px}}
.btn{{flex:1;height:44px;border-radius:12px;font-size:14px;font-weight:600;cursor:pointer;
border:1px solid rgba(20,20,50,.16);background:rgba(255,255,255,.6);color:#0b0b10;transition:.15s}}
.btn:hover{{background:rgba(255,255,255,.9)}}
.btn.primary{{background:#0b0b10;border-color:#0b0b10;color:#fff}}
.btn.primary:hover{{background:#26262e}}
.btn.primary:disabled{{opacity:.25;cursor:not-allowed}}
.foot{{margin-top:22px;padding-top:14px;border-top:1px solid rgba(20,20,40,.08);font:400 11px/1.7 ui-monospace,Menlo,monospace;color:#9aa0b5;text-align:center}}
@media (prefers-reduced-motion:reduce){{.wire::after,.card,.err{{animation:none}}}}
</style></head><body>
<canvas id="gl"></canvas>
<div class="noise"></div>
<div class="brand">MCP<b>·</b>BRIDGE</div>
<div class="wrap"><div class="card">
  <div class="tag">AUTHORIZATION — STEP 01/02</div>
  <h1>输入授权码</h1>
  <div class="sub">桌面软件 MCP Bridge 已弹出授权窗口,将其中的 8 位配对码输入下方,即可完成 ChatGPT 与本机 IDE 的安全连接。</div>
  <div class="link">
    <div class="node"><div class="ic"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#0b0b10" stroke-width="1.8" stroke-linecap="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg></div>本机 IDE</div>
    <div class="wire"></div>
    <div class="node"><div class="ic"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#0b0b10" stroke-width="1.8" stroke-linecap="round"><path d="M3 17c3-4 6-6 9-6s6 2 9 6"/><path d="M3 17v3M21 17v3M12 11V5"/></svg></div>Bridge</div>
    <div class="wire"></div>
    <div class="node"><div class="ic"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#0b0b10" stroke-width="1.8" stroke-linejoin="round"><path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"/><path d="M19 15l.9 2.6L22.5 18.5l-2.6.9L19 22l-.9-2.6-2.6-.9 2.6-.9z"/></svg></div>ChatGPT</div>
  </div>
  <form method="post" action="/oauth/authorize" id="f">
    {hidden}
    <input class="code" id="code" name="pairing_code" placeholder="XXXX-XXXX" maxlength="9" autocomplete="off" autofocus>
    {err_html}
    <div class="row">
      <button type="submit" class="btn" name="decision" value="deny" formnovalidate>拒绝</button>
      <button type="submit" class="btn primary" id="ok" name="decision" value="allow" disabled>允许访问 IDE</button>
    </div>
  </form>
  <div class="foot">配对码 120 秒内有效 · 一次性 · 仅你本人发起的流程</div>
</div></div>
<script>
const i=document.getElementById('code'),ok=document.getElementById('ok');
i.addEventListener('input',()=>{{
  let v=i.value.toUpperCase().replace(/[^A-Z0-9]/g,'').slice(0,8);
  i.value=v.length>4?v.slice(0,4)+'-'+v.slice(4):v;
  ok.disabled=v.length<8;
}});
</script>
<script>{aurora_js}</script>
<script>createAurora(document.getElementById('gl'));</script>
</body></html>"##,
        hidden = hidden,
        err_html = err_html,
        aurora_js = AURORA_JS
    )
}

fn render_error(msg: &str) -> String {
    format!(
        r##"<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><title>MCP Bridge · 授权</title>
<style>body{{min-height:100vh;display:flex;align-items:center;justify-content:center;background:#fafafa;font-family:-apple-system,"PingFang SC",sans-serif;background-image:radial-gradient(#d4d4d4 1px,transparent 1px);background-size:22px 22px}}
.card{{width:400px;background:#fff;border:1px solid #e5e5e5;border-radius:16px;padding:30px;text-align:center}}
h1{{font-size:18px;margin-bottom:8px}}p{{font-size:13px;color:#6E6E73;line-height:1.6}}</style></head>
<body><div class="card"><h1>授权请求无效</h1><p>{}</p></div></body></html>"##,
        html_escape(msg)
    )
}
