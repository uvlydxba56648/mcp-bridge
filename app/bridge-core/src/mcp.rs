//! JetBrains MCP 上游:Streamable HTTP 握手、会话复用、工具列表缓存、工具调用转发。
//!
//! 性能要点:
//! - `reqwest::Client` 连接池 + keep-alive,回环地址上每次调用只花 ~1ms RTT
//! - 上游 MCP session 只 initialize 一次,后续复用 `Mcp-Session-Id`
//! - `tools/list` 结果常驻缓存,网关直接命中(0 上游往返)
//! - 上游返回 SSE 时按帧解析,取到匹配 id 的 JSON-RPC 响应即返回,不整段缓冲

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub endpoint: String,
    pub transport: String,
    pub tool_count: usize,
    pub tools: Vec<ToolDef>,
    pub probe_ms: u128,
}

fn base_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, HeaderValue::from_static("application/json, text/event-stream"));
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h
}

/// 从 SSE 帧文本中提取匹配 `id` 的 JSON-RPC 响应
fn extract_jsonrpc_response(body: &str, id: i64) -> Option<Value> {
    for frame in body.split("\n\n") {
        for line in frame.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                    if v.get("id").and_then(Value::as_i64) == Some(id)
                        && (v.get("result").is_some() || v.get("error").is_some())
                    {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

/// 流式读取 SSE 响应,直到拿到匹配 id 的响应帧
async fn read_sse_response(mut stream: impl futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin, id: i64) -> Result<Value> {
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk?));
        if let Some(v) = extract_jsonrpc_response(&buf, id) {
            return Ok(v);
        }
        if buf.len() > 4 * 1024 * 1024 {
            bail!("上游 SSE 响应超过 4MB 仍未完成");
        }
    }
    bail!("上游连接关闭,未收到 id={} 的响应", id)
}

/// 探测单个 endpoint 是否为可用的 MCP(Streamable HTTP)
pub async fn probe_endpoint(client: &reqwest::Client, url: &str) -> Result<ServerInfo> {
    let t0 = Instant::now();
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "mcp-bridge", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    let resp = client.post(url).headers(base_headers()).json(&body).send().await?;
    if !resp.status().is_success() {
        bail!("initialize 返回 {}", resp.status());
    }
    let session = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let text = resp.text().await?;
    let init_resp = if ct.contains("text/event-stream") {
        extract_jsonrpc_response(&text, 1).ok_or_else(|| anyhow!("SSE 中无 initialize 响应"))?
    } else {
        serde_json::from_str(&text).context("initialize 响应非 JSON")?
    };
    let server = init_resp
        .pointer("/result/serverInfo")
        .cloned()
        .unwrap_or(json!({"name": "unknown", "version": "?"}));

    // notifications/initialized
    let mut req = client.post(url).headers(base_headers());
    if let Some(s) = &session {
        req = req.header("mcp-session-id", s);
    }
    let _ = req
        .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .send()
        .await;

    // tools/list
    let mut req = client.post(url).headers(base_headers());
    if let Some(s) = &session {
        req = req.header("mcp-session-id", s);
    }
    let resp = req
        .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
        .send()
        .await?;
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let tools_resp = if ct.contains("text/event-stream") {
        read_sse_response(resp.bytes_stream(), 2).await?
    } else {
        resp.json::<Value>().await?
    };
    let tools: Vec<ToolDef> =
        serde_json::from_value(tools_resp.pointer("/result/tools").cloned().unwrap_or(json!([])))
            .unwrap_or_default();

    Ok(ServerInfo {
        name: server["name"].as_str().unwrap_or("unknown").into(),
        version: server["version"].as_str().unwrap_or("?").into(),
        endpoint: url.into(),
        transport: "streamable-http".into(),
        tool_count: tools.len(),
        tools,
        probe_ms: t0.elapsed().as_millis(),
    })
}

/// 并行探测常见端口/路径,返回最先成功的结果(降低首屏等待)
pub async fn probe_local() -> Result<ServerInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()?;
    let mut tasks = Vec::new();
    for port in [64342u16, 63342] {
        for path in ["/stream", "/sse"] {
            let url = format!("http://127.0.0.1:{}{}", port, path);
            let c = client.clone();
            tasks.push(tokio::spawn(async move { probe_endpoint(&c, &url).await }));
        }
    }
    let mut last_err = anyhow!("无可用端点");
    for t in tasks {
        match t.await {
            Ok(Ok(info)) => return Ok(info),
            Ok(Err(e)) => last_err = e,
            Err(e) => last_err = e.into(),
        }
    }
    Err(last_err).context("未检测到本机 JetBrains MCP(请确认 IDE 已开启 MCP Server)")
}

/// 手动粘贴配置的探测:
/// - streamable-http:直接探测该 URL
/// - sse:JetBrains 同一端口同时提供 /stream,自动映射后探测(上游统一走 streamable)
pub async fn probe_manual(url: &str, kind: &str) -> Result<ServerInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1500))
        .build()?;
    if kind == "sse" {
        let stream_url = if url.ends_with("/sse") {
            url[..url.len() - 4].to_string() + "/stream"
        } else {
            url.trim_end_matches('/').to_string() + "/stream"
        };
        let mut info = probe_endpoint(&client, &stream_url)
            .await
            .with_context(|| format!("SSE 配置自动映射到 {} 后握手失败", stream_url))?;
        info.transport = "sse(上游走 streamable-http)".into();
        return Ok(info);
    }
    probe_endpoint(&client, url).await
}

/// 常驻上游:会话复用 + 工具缓存 + 调用转发
pub struct Upstream {
    client: reqwest::Client,
    url: String,
    session: RwLock<Option<String>>,
    tools_cache: RwLock<Option<Arc<Vec<ToolDef>>>>,
    /// 默认 projectPath:ChatGPT 不知道本机路径,网关自动注入
    default_project: RwLock<Option<String>>,
}

impl Upstream {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(8)
            .tcp_keepalive(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self {
            client,
            url: url.into(),
            session: RwLock::new(None),
            tools_cache: RwLock::new(None),
            default_project: RwLock::new(None),
        }
    }

    pub async fn set_default_project(&self, p: Option<String>) {
        *self.default_project.write().await = p;
    }

    async fn post_rpc(&self, body: Value, session: Option<&str>) -> Result<reqwest::Response> {
        let mut req = self.client.post(&self.url).headers(base_headers());
        if let Some(s) = session {
            req = req.header("mcp-session-id", s);
        }
        Ok(req.json(&body).send().await?)
    }

    /// 建立(或重建)上游会话;返回 session id
    async fn init_session(&self) -> Result<Option<String>> {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "mcp-bridge-gateway", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        let resp = self.post_rpc(body, None).await?;
        if !resp.status().is_success() {
            bail!("上游 initialize 失败: {}", resp.status());
        }
        let session = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        drop(resp);
        let _ = self
            .post_rpc(json!({"jsonrpc":"2.0","method":"notifications/initialized"}), session.as_deref())
            .await;
        *self.session.write().await = session.clone();
        Ok(session)
    }

    async fn ensure_session(&self) -> Result<Option<String>> {
        if let Some(s) = self.session.read().await.clone() {
            return Ok(Some(s));
        }
        self.init_session().await
    }

    /// 工具列表:优先命中缓存
    pub async fn tools(&self, force_refresh: bool) -> Result<Arc<Vec<ToolDef>>> {
        if !force_refresh {
            if let Some(t) = self.tools_cache.read().await.clone() {
                return Ok(t);
            }
        }
        let session = self.ensure_session().await?;
        let resp = self
            .post_rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), session.as_deref())
            .await?;
        let v = read_jsonrpc(resp, 2).await?;
        let tools: Vec<ToolDef> = serde_json::from_value(
            v.pointer("/result/tools").cloned().unwrap_or(json!([])),
        )?;
        let arc = Arc::new(tools);
        *self.tools_cache.write().await = Some(arc.clone());
        Ok(arc)
    }

    /// 调用工具:复用会话;缺 projectPath 时注入默认;会话失效(404)时自动重建并重试一次
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let mut arguments = arguments;
        if let Some(obj) = arguments.as_object_mut() {
            if !obj.contains_key("projectPath") {
                if let Some(dp) = self.default_project.read().await.clone() {
                    obj.insert("projectPath".into(), Value::String(dp));
                }
            }
        }
        let body = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        let session = self.ensure_session().await?;
        let resp = self.post_rpc(body.clone(), session.as_deref()).await?;
        if resp.status().as_u16() == 404 {
            // 上游会话过期 → 重建并重试
            let session = self.init_session().await?;
            let resp = self.post_rpc(body, session.as_deref()).await?;
            return read_jsonrpc(resp, 3).await;
        }
        read_jsonrpc(resp, 3).await
    }
}

/// 读 JSON-RPC 响应:兼容 application/json 与 text/event-stream
async fn read_jsonrpc(resp: reqwest::Response, id: i64) -> Result<Value> {
    if !resp.status().is_success() {
        bail!("上游返回 {}", resp.status());
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if ct.contains("text/event-stream") {
        read_sse_response(resp.bytes_stream(), id).await
    } else {
        Ok(resp.json::<Value>().await?)
    }
}
