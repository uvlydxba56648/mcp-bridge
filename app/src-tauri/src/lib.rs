//! MCP Bridge — Tauri 命令层(薄壳,逻辑全在 bridge-core)

use bridge_core::{gateway, logbus::{LogBus, PairBus}, mcp, tunnel};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

struct Inner {
    upstream: Option<Arc<mcp::Upstream>>,
    gateway: Option<gateway::GatewayHandle>,
    tunnel: Option<tunnel::TunnelHandle>,
    public_url: Option<String>,
    tunnel_id: Option<String>,
    allow_noauth: bool,
}

struct BridgeState {
    inner: Mutex<Inner>,
    log: LogBus,
    pair: PairBus,
    oauth: Arc<bridge_core::oauth::OAuthServer>,
}

/// 连接成功后预热:上游会话 + tools/list 缓存 + 启动鉴权网关(幂等)
async fn prewarm(state: &BridgeState, endpoint: &str) {
    let mut g = state.inner.lock().await;
    if g.gateway.is_some() {
        return;
    }
    let upstream = Arc::new(mcp::Upstream::new(endpoint.to_string()));
    let _ = upstream.tools(false).await;
    match gateway::start(upstream.clone(), state.oauth.clone(), state.log.clone(), state.pair.clone(), 0, g.allow_noauth).await {
        Ok(h) => {
            g.gateway = Some(h);
            g.upstream = Some(upstream);
        }
        Err(e) => state.log.emit("ERR", "网关", format!("网关启动失败: {}", e)),
    }
}

#[derive(Serialize)]
struct TunnelStatus {
    installed: bool,
    provider: String,
    url: Option<String>,
    gateway_port: Option<u16>,
    tunnel_id: Option<String>,
}

/// S1: 探测本机 MCP;成功后立即预热上游会话 + 启动鉴权网关(后台零等待)
#[tauri::command]
async fn detect_mcp(state: State<'_, BridgeState>) -> Result<mcp::ServerInfo, String> {
    let info = mcp::probe_local().await.map_err(|e| e.to_string())?;
    state.log.emit(
        "OK",
        "检测",
        format!("已连接 {} {} @ {}({} ms)", info.name, info.version, info.endpoint, info.probe_ms),
    );
    state.log.emit("INFO", "检测", format!("发现 {} 个工具,全部放开", info.tool_count));
    prewarm(&state, &info.endpoint).await;
    Ok(info)
}

/// S1: 手动粘贴配置后真正去握手验证(SSE 配置自动映射到 /stream),成功后同样预热
#[tauri::command]
async fn probe_manual(state: State<'_, BridgeState>, url: String, kind: String) -> Result<mcp::ServerInfo, String> {
    let info = mcp::probe_manual(&url, &kind).await.map_err(|e| e.to_string())?;
    state.log.emit(
        "OK",
        "检测",
        format!("手动配置握手成功: {} {}({} 个工具)", info.name, info.version, info.tool_count),
    );
    prewarm(&state, &info.endpoint).await;
    Ok(info)
}

/// S2: 确保隧道组件(系统有就用系统的,否则自动下载)并建立隧道
#[tauri::command]
async fn ensure_tunnel(
    state: State<'_, BridgeState>,
    provider: String,
    token: Option<String>,
    tunnel_id: Option<String>,
) -> Result<TunnelStatus, String> {
    let p = tunnel::Provider::from_id(&provider).map_err(|e| e.to_string())?;
    let bin = tunnel::ensure(p, &state.log).await.map_err(|e| e.to_string())?;

    let mut g = state.inner.lock().await;
    let gw_port = g.gateway.as_ref().map(|h| h.port).ok_or("网关未启动,请先完成第 1 步")?;

    // OpenAI Tunnel 无公网入口,鉴权在 OpenAI 平台侧 → 网关切到免 Key 模式
    let need_noauth = p == tunnel::Provider::OpenAi;
    if need_noauth != g.allow_noauth {
        g.allow_noauth = need_noauth;
        restart_gateway(&mut g, &state.oauth, &state.pair, &state.log).await;
    }

    if g.tunnel.is_none() {
        let creds = tunnel::Creds { token, tunnel_id: tunnel_id.clone() };
        let handle = tunnel::start(p, &bin, gw_port, &creds, &state.log)
            .await
            .map_err(|e| e.to_string())?;
        g.public_url = handle.url.clone().map(|u| format!("{}/mcp", u));
        g.tunnel_id = tunnel_id;
        g.tunnel = Some(handle);

        // 端到端自检:通过公网 URL 回环调用 initialize
        // (新 trycloudflare 域名的 DNS 需几秒传播 → 退避重试;OpenAI 模式无公网 URL,跳过)
        if let Some(url) = g.public_url.clone() {
            let key = state.oauth.mint_local(None);
            let log2 = state.log.clone();
            tauri::async_runtime::spawn(async move {
                // 直连 + 系统代理两个客户端;翻墙用户本机回环经常要代理才通
                let clients = [
                    ("直连", bridge_core::netutil::client(std::time::Duration::from_secs(15))),
                    ("代理", bridge_core::netutil::proxied_client(std::time::Duration::from_secs(15))),
                ];
                let has_proxy = bridge_core::netutil::system_proxy().is_some();
                for attempt in 1..=10u8 {
                    for (label, client) in clients.iter() {
                        if *label == "代理" && !has_proxy {
                            continue;
                        }
                        let t0 = std::time::Instant::now();
                        let resp = client.post(&url)
                            .header("Authorization", format!("Bearer {}", key))
                            .header("Accept", "application/json, text/event-stream")
                            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
                                "params":{"protocolVersion":"2025-03-26","capabilities":{},
                                          "clientInfo":{"name":"mcp-bridge-selfcheck","version":"0"}}}))
                            .send().await;
                        match resp {
                            Ok(r) if r.status().is_success() => {
                                log2.emit("OK", "自检", format!("经公网地址回环调用成功({} ms,{})", t0.elapsed().as_millis(), label));
                                return;
                            }
                            Ok(r) => log2.emit("WARN", "自检", format!("公网回环返回 {}({},第 {}/10 次)", r.status(), label, attempt)),
                            Err(e) => log2.emit("WARN", "自检", format!("公网回环失败({},{},第 {}/10 次)", e, label, attempt)),
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                }
                log2.emit("WARN", "自检", "公网回环多次失败。若你使用代理/翻墙:本机回环不通不代表 ChatGPT 连不上(ChatGPT 从海外访问),可直接去配置;也可换 ngrok / OpenAI Tunnel");
            });
        }
    }

    Ok(TunnelStatus {
        installed: true,
        provider,
        url: g.public_url.clone(),
        gateway_port: Some(gw_port),
        tunnel_id: g.tunnel_id.clone(),
    })
}

/// 同端口重启网关(换 key / 切免鉴权模式时用,隧道指向不变)
async fn restart_gateway(g: &mut Inner, oauth: &Arc<bridge_core::oauth::OAuthServer>, pair: &PairBus, log: &LogBus) {
    if let (Some(h), Some(up)) = (g.gateway.take(), g.upstream.clone()) {
        let port = h.port;
        h.stop();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(h2) = gateway::start(up, oauth.clone(), log.clone(), pair.clone(), port, g.allow_noauth).await {
            g.gateway = Some(h2);
            log.emit("INFO", "网关", format!("网关已在端口 {} 重启", port));
        }
    }
}

#[tauri::command]
async fn close_connection(state: State<'_, BridgeState>) -> Result<(), String> {
    let mut g = state.inner.lock().await;
    if let Some(t) = g.tunnel.take() {
        t.stop().await;
    }
    g.public_url = None;
    state.log.emit("INFO", "网关", "连接已关闭,隧道已断开");
    Ok(())
}

/// 卸载我们自动安装的隧道组件(带 .managed 标记的才删,系统装的一概不动)
#[tauri::command]
async fn cleanup_components(state: State<'_, BridgeState>) -> Result<(), String> {
    {
        let mut g = state.inner.lock().await;
        if let Some(t) = g.tunnel.take() {
            t.stop().await;
        }
        g.public_url = None;
    }
    for p in [tunnel::Provider::Cloudflare, tunnel::Provider::Ngrok, tunnel::Provider::OpenAi] {
        let _ = tunnel::cleanup(p, &state.log).await;
    }
    Ok(())
}

/// 后台运行:只隐藏窗口,网关/隧道保持在线
#[tauri::command]
async fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    Ok(())
}

/// 直接退出:先杀隧道子进程(tokio Child 默认不随退出杀死,会变孤儿),再退出
#[tauri::command]
async fn quit_app(app: AppHandle) -> Result<(), String> {
    let state = app.state::<BridgeState>();
    {
        let mut g = state.inner.lock().await;
        if let Some(t) = g.tunnel.take() {
            t.stop().await;
        }
        if let Some(h) = g.gateway.take() {
            h.stop();
        }
    }
    app.exit(0);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 点窗口关闭按钮 → 不直接关,发事件让前端弹窗选择「后台运行 / 直接退出」
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // ── 系统托盘(Windows 隐藏图标区 / macOS 顶部菜单栏) ──
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
            let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 Exit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("MCP Bridge")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        // 托盘退出同样要先杀隧道子进程
                        let app2 = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app2.state::<BridgeState>();
                            {
                                let mut g = state.inner.lock().await;
                                if let Some(t) = g.tunnel.take() {
                                    t.stop().await;
                                }
                                if let Some(h) = g.gateway.take() {
                                    h.stop();
                                }
                            }
                            app2.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;
            // 托盘句柄不能 drop,否则图标消失
            Box::leak(Box::new(tray));

            let log = LogBus::new();
            // 日志事件 → 前端日志窗口
            let mut rx = log.subscribe();
            let handle: AppHandle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(ev) = rx.recv().await {
                    let _ = handle.emit("log", ev);
                }
            });
            // 配对码事件 → 前端弹窗
            let pair = PairBus::new();
            let mut prx = pair.subscribe();
            let handle2: AppHandle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(code) = prx.recv().await {
                    let _ = handle2.emit("pairing", code);
                }
            });
            app.manage(BridgeState {
                inner: Mutex::new(Inner {
                    upstream: None,
                    gateway: None,
                    tunnel: None,
                    public_url: None,
                    tunnel_id: None,
                    allow_noauth: false,
                }),
                log,
                pair,
                oauth: Arc::new(bridge_core::oauth::OAuthServer::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_mcp,
            probe_manual,
            ensure_tunnel,
            close_connection,
            cleanup_components,
            hide_window,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
