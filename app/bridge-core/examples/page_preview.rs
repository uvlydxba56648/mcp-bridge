//! 授权页预览:cargo run --example page_preview 后访问打印的 URL
use bridge_core::{gateway, logbus::{LogBus, PairBus}, mcp::Upstream, oauth::OAuthServer};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let log = LogBus::new();
    let oauth = Arc::new(OAuthServer::new());
    // 预注册一个客户端,伪造授权请求参数
    let reg = oauth.register(vec!["https://chatgpt.com/connector_platform_oauth_redirect".into()]);
    let cid = reg["client_id"].as_str().unwrap();
    let upstream = Arc::new(Upstream::new("http://127.0.0.1:64342/stream"));
    let gw = gateway::start(upstream, oauth, log, PairBus::new(), 0, false).await?;
    println!("URL: http://127.0.0.1:{}/oauth/authorize?response_type=code&client_id={}&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector_platform_oauth_redirect&state=demo&scope=mcp&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&resource=http%3A%2F%2F127.0.0.1%3A{}", gw.port, cid, gw.port);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
