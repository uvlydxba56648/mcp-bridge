//! 网络工具:系统代理探测(环境变量 → Windows 注册表 → macOS scutil)+ reqwest 客户端工厂。
//! 面向翻墙/公司代理用户:直连失败时调用方可用 proxied_client 再试。

use std::sync::OnceLock;
use std::time::Duration;

static PROXY: OnceLock<Option<String>> = OnceLock::new();

/// Windows 下拉起 reg/scutil 也不能弹 cmd 黑窗
#[allow(dead_code)]
fn silent_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn normalize(v: String) -> Option<String> {
    let v = v.trim().to_string();
    if v.is_empty() {
        return None;
    }
    if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("socks") {
        Some(v)
    } else {
        Some(format!("http://{}", v))
    }
}

fn from_env() -> Option<String> {
    for k in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(k) {
            if let Some(p) = normalize(v) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(windows)]
fn from_os() -> Option<String> {
    // ProxyEnable 与 ProxyServer 在 HKCU\...\Internet Settings
    let q = |v: &str| -> Option<String> {
        let out = silent_std(std::process::Command::new("reg").args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            v,
        ]))
        .output()
        .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        s.lines().find(|l| l.contains(v)).map(|l| l.to_string())
    };
    let enable = q("ProxyEnable")?;
    if !enable.contains("0x1") {
        return None;
    }
    let server = q("ProxyServer")?;
    let value = server.split_whitespace().last()?.to_string();
    // 形如 http=127.0.0.1:7890;https=127.0.0.1:7890 或 127.0.0.1:7890
    for part in value.split(';') {
        if let Some(rest) = part.strip_prefix("https=") {
            return normalize(rest.to_string());
        }
    }
    normalize(value.split('=').last().unwrap_or(&value).to_string())
}

#[cfg(target_os = "macos")]
fn from_os() -> Option<String> {
    let out = silent_std(std::process::Command::new("scutil").arg("--proxy")).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let get = |k: &str| -> Option<String> {
        s.lines()
            .find(|l| l.trim_start().starts_with(k))
            .and_then(|l| l.split(':').nth(1))
            .map(|v| v.trim().to_string())
    };
    if get("HTTPEnable").as_deref() != Some("1") {
        return None;
    }
    let host = get("HTTPProxy")?;
    let port = get("HTTPPort").unwrap_or_else(|| "80".into());
    normalize(format!("{}:{}", host, port))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn from_os() -> Option<String> {
    None
}

/// 探测到的系统代理("http://host:port"),缓存一次
pub fn system_proxy() -> Option<&'static str> {
    PROXY.get_or_init(|| from_env().or_else(from_os)).as_deref()
}

/// 直连客户端
pub fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

/// 带系统代理的客户端(无代理时退化为直连)
pub fn proxied_client(timeout: Duration) -> reqwest::Client {
    let mut b = reqwest::Client::builder().timeout(timeout);
    if let Some(p) = system_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(p) {
            b = b.proxy(proxy);
        }
    }
    b.build().unwrap_or_default()
}
