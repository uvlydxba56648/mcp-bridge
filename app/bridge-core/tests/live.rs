//! 对本机真实 JetBrains MCP 的集成测试。
//! 运行: cargo test -- --ignored --nocapture
//! 前提: IDE 已开启 MCP Server(127.0.0.1:64342 或 63342)

use bridge_core::{gateway, logbus::{LogBus, PairBus}, mcp, oauth::OAuthServer};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
#[ignore = "需要本机 IDE MCP 在线"]
async fn probe_live() {
    let info = mcp::probe_local().await.expect("探测失败");
    eprintln!("探测成功: {} {} @ {} ({} ms, {} 工具)",
        info.name, info.version, info.endpoint, info.probe_ms, info.tool_count);
    assert!(info.tool_count > 0);
}

/// OAuth 全流程:DCR → 授权页 → PKCE 换 token → 带 token 调 /mcp → 401 校验
#[tokio::test]
#[ignore = "需要本机 IDE MCP 在线"]
async fn gateway_oauth_live() {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let log = LogBus::new();
    let info = mcp::probe_local().await.expect("探测失败");
    let upstream = Arc::new(mcp::Upstream::new(info.endpoint.clone()));
    let oauth = Arc::new(OAuthServer::new());
    let pair = PairBus::new();
    let mut pair_rx = pair.subscribe();
    let gw = gateway::start(upstream.clone(), oauth.clone(), log.clone(), pair.clone(), 0, false)
        .await
        .expect("网关启动失败");
    let base = format!("http://127.0.0.1:{}", gw.port);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // 0) 无 token → 401 + WWW-Authenticate
    let r = client.post(format!("{}/mcp", base))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send().await.unwrap();
    assert_eq!(r.status(), 401);
    let wa = r.headers().get("www-authenticate").unwrap().to_str().unwrap().to_string();
    assert!(wa.contains("oauth-protected-resource"), "缺少 RFC9728 挑战头: {}", wa);

    // 1) well-known 元数据
    let r: serde_json::Value = client.get(format!("{}/.well-known/oauth-protected-resource", base)).send().await.unwrap().json().await.unwrap();
    assert!(r["authorization_servers"].is_array());
    let r: serde_json::Value = client.get(format!("{}/.well-known/oauth-authorization-server", base)).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["code_challenge_methods_supported"][0], "S256");

    // 2) DCR 注册
    let redirect_uri = "https://chatgpt.com/connector_platform_oauth_redirect";
    let r = client.post(format!("{}/oauth/register", base))
        .json(&json!({"redirect_uris": [redirect_uri], "client_name": "test"}))
        .send().await.unwrap();
    assert_eq!(r.status(), 201);
    let reg: serde_json::Value = r.json().await.unwrap();
    let client_id = reg["client_id"].as_str().unwrap().to_string();

    // 3) PKCE 授权码流程
    let verifier = "test-verifier-0123456789-0123456789-0123456789";
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let authz = format!(
        "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&state=s1&scope=mcp&code_challenge={}&code_challenge_method=S256&resource={}",
        base, client_id, urlencoding(redirect_uri), challenge, urlencoding(&base)
    );
    // GET 授权页(触发配对码)
    let r = client.get(&authz).send().await.unwrap();
    assert_eq!(r.status(), 200, "授权页未渲染");
    let pairing = tokio::time::timeout(std::time::Duration::from_secs(2), pair_rx.recv()).await.unwrap().unwrap();
    assert!(pairing.len() == 9 && pairing.contains('-'), "配对码格式错误: {}", pairing);
    // POST 允许 → 302 带 code + iss
    let form: Vec<(&str, &str)> = vec![
        ("decision", "allow"), ("response_type", "code"), ("client_id", &client_id),
        ("redirect_uri", redirect_uri), ("state", "s1"), ("scope", "mcp"),
        ("code_challenge", &challenge), ("code_challenge_method", "S256"), ("resource", &base),
        ("pairing_code", &pairing),
    ];
    let r = client.post(format!("{}/oauth/authorize", base)).form(&form).send().await.unwrap();
    assert_eq!(r.status(), 303, "期望重定向,得到 {}", r.status());
    let loc = r.headers().get("location").unwrap().to_str().unwrap().to_string();
    assert!(loc.contains("code=") && loc.contains("iss="), "重定向缺少 code/iss: {}", loc);
    let code = loc.split("code=").nth(1).unwrap().split('&').next().unwrap().to_string();

    // 4) 换 token
    let r = client.post(format!("{}/oauth/token", base))
        .form(&[
            ("grant_type", "authorization_code"), ("code", code.as_str()),
            ("client_id", client_id.as_str()), ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send().await.unwrap();
    let tk: serde_json::Value = r.json().await.unwrap();
    let at = tk["access_token"].as_str().unwrap().to_string();
    let rt = tk["refresh_token"].as_str().unwrap().to_string();
    assert!(!at.is_empty());

    // 5) 带 token 调 /mcp → tools/list 命中缓存
    let r = client.post(format!("{}/mcp", base))
        .header("Authorization", format!("Bearer {}", at))
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send().await.unwrap();
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v.pointer("/result/tools").unwrap().as_array().unwrap().len(), info.tool_count);

    // 6) refresh 轮换:旧 rt 用两次,第二次必须失败
    let r = client.post(format!("{}/oauth/token", base))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", rt.as_str())])
        .send().await.unwrap();
    assert!(r.status().is_success());
    let r = client.post(format!("{}/oauth/token", base))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", rt.as_str())])
        .send().await.unwrap();
    assert_eq!(r.status(), 400, "旧 refresh token 未被轮换作废");

    eprintln!("OAuth 全流程通过: DCR → authorize → PKCE token → /mcp → refresh 轮换");
    gw.stop();
}

fn urlencoding(s: &str) -> String {
    s.replace(':', "%3A").replace('/', "%2F")
}
