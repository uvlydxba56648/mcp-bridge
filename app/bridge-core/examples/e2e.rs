//! 端到端真链路:探测 → 预热 → 网关 → 安装 cloudflared → quick tunnel → 公网自检
//! 运行: cargo run --example e2e

use bridge_core::{gateway, logbus::{LogBus, PairBus}, mcp, tunnel};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();
    let log = LogBus::new();
    let mut rx = log.subscribe();
    tokio::spawn(async move {
        while let Ok(e) = rx.recv().await {
            println!("[{}] {:4} {:3} │ {}", e.time, e.level, e.source, e.message);
        }
    });

    println!("── 1. 探测本机 MCP ──");
    let info = mcp::probe_local().await?;
    println!("   {} {} @ {} · {} 工具 · {} ms\n", info.name, info.version, info.endpoint, info.tool_count, info.probe_ms);

    println!("── 2. 预热上游会话 + 工具缓存,启动鉴权网关 ──");
    let upstream = Arc::new(mcp::Upstream::new(info.endpoint.clone()));
    let t0 = std::time::Instant::now();
    upstream.tools(false).await?;
    println!("   预热 tools/list: {} ms", t0.elapsed().as_millis());

    let oauth = std::sync::Arc::new(bridge_core::oauth::OAuthServer::new());
    let gw = gateway::start(upstream.clone(), oauth.clone(), log.clone(), PairBus::new(), 0, false).await?;
    let key = oauth.mint_local(None);
    println!("   网关端口: {} · 自检 token: {}…\n", gw.port, &key[..16]);

    println!("── 3. 确保 cloudflared(系统有优先,缺失则自动下载) ──");
    let bin = tunnel::ensure(tunnel::Provider::Cloudflare, &log).await?;
    println!("   {:?}\n", bin);

    println!("── 4. 建立 quick tunnel ──");
    let t = tunnel::start(tunnel::Provider::Cloudflare, &bin, gw.port, &tunnel::Creds::default(), &log).await?;
    let public = format!("{}/mcp", t.url.as_deref().unwrap_or(""));
    println!("   公网地址: {}\n", public);

    println!("── 5. 端到端自检:经公网 URL 回环调用(DNS 传播需时间,退避重试) ──");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()?;
    let mut v: serde_json::Value = serde_json::Value::Null;
    let mut elapsed = 0;
    for attempt in 1..=12 {
        let t0 = std::time::Instant::now();
        match client
            .post(&public)
            .header("Authorization", format!("Bearer {}", key))
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                elapsed = t0.elapsed().as_millis();
                v = r.json().await?;
                break;
            }
            Ok(r) => println!("   第 {} 次:HTTP {},4s 后重试…", attempt, r.status()),
            Err(e) => println!("   第 {} 次:{},4s 后重试…", attempt, e),
        }
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    }
    let n = v.pointer("/result/tools").and_then(|t| t.as_array()).map(|a| a.len()).unwrap_or(0);
    println!("   公网 tools/list: {} 个工具 · {} ms", n, elapsed);
    assert!(n == info.tool_count, "公网自检失败");

    let t0 = std::time::Instant::now();
    let r = client
        .post(&public)
        .header("Authorization", format!("Bearer {}", key))
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                      "params":{"name":"get_all_open_file_paths","arguments":{}}}))
        .send()
        .await?;
    let v: serde_json::Value = r.json().await?;
    let preview = v.pointer("/result/content/0/text").and_then(|x| x.as_str()).unwrap_or("").chars().take(100).collect::<String>();
    println!("   公网 tools/call get_all_open_file_paths: {} ms\n   → {}", t0.elapsed().as_millis(), preview);

    // 鉴权验证:无 token 必须 401 且带 RFC9728 挑战头
    let r401 = client.post(&public).json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})).send().await?;
    println!("   无 token → HTTP {} (预期 401) · WWW-Authenticate: {}",
        r401.status(),
        r401.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).unwrap_or("无"));
    assert_eq!(r401.status(), 401);

    println!("\n── 收尾:断开隧道 ──");
    t.stop().await;
    gw.stop();
    println!("完成 ✅");
    Ok(())
}
