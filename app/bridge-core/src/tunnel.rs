//! 隧道组件托管:Cloudflare quick tunnel / ngrok / OpenAI tunnel-client
//! - 系统已安装的二进制优先(PATH),不重复安装
//! - 我们自动下载的安装到独立目录并写 .managed 标记;cleanup() 只清理带标记的,
//!   绝不动系统 PATH 里的组件

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::logbus::LogBus;

/// Windows 下 GUI 应用拉起控制台子进程会闪出 cmd 黑窗 → CREATE_NO_WINDOW
fn silent(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd
}

fn silent_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

/// GitHub 镜像候选(直连 → ghproxy → gh-proxy),适配不同网络环境
fn mirror_candidates(url: &str) -> Vec<String> {
    if !url.starts_with("https://github.com") {
        return vec![url.to_string()];
    }
    vec![
        url.to_string(),
        format!("https://mirror.ghproxy.com/{}", url),
        format!("https://gh-proxy.com/{}", url),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Cloudflare,
    Ngrok,
    OpenAi,
}

impl Provider {
    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "cloudflare" => Ok(Self::Cloudflare),
            "ngrok" => Ok(Self::Ngrok),
            "openai" => Ok(Self::OpenAi),
            other => bail!("未知隧道方案: {}", other),
        }
    }

    pub fn binary_name(self) -> &'static str {
        let win = cfg!(windows);
        match self {
            Self::Cloudflare => {
                if win { "cloudflared.exe" } else { "cloudflared" }
            }
            Self::Ngrok => {
                if win { "ngrok.exe" } else { "ngrok" }
            }
            Self::OpenAi => {
                if win { "tunnel-client.exe" } else { "tunnel-client" }
            }
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::Cloudflare => "cloudflared",
            Self::Ngrok => "ngrok",
            Self::OpenAi => "tunnel-client",
        }
    }

    /// 组件安装目录: ~/.local/share/mcp-bridge/bin/<provider>/(含 .managed 标记)
    fn dir(self) -> PathBuf {
        install_root().join(match self {
            Self::Cloudflare => "cloudflare",
            Self::Ngrok => "ngrok",
            Self::OpenAi => "openai",
        })
    }

    fn marker(self) -> PathBuf {
        self.dir().join(".managed")
    }
}

/// 凭据:cf=token(可选/named)、ngrok=authtoken、openai=api key + tunnel_id
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Creds {
    pub token: Option<String>,
    pub tunnel_id: Option<String>,
}

fn install_root() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("mcp-bridge").join("bin")
}

/// 系统 PATH 查找
fn which(name: &str) -> Option<PathBuf> {
    let cmd = if cfg!(windows) { "where" } else { "which" };
    let out = silent_std(std::process::Command::new(cmd).arg(name)).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}

/// 系统已装的优先,其次我们装的
pub fn find(p: Provider) -> Option<PathBuf> {
    if let Some(sys) = which(p.binary_name()) {
        return Some(sys);
    }
    let local = p.dir().join(p.binary_name());
    local.exists().then_some(local)
}

fn platform() -> Result<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok(("darwin", "arm64")),
        ("macos", "x86_64") => Ok(("darwin", "amd64")),
        ("linux", "x86_64") => Ok(("linux", "amd64")),
        ("linux", "aarch64") => Ok(("linux", "arm64")),
        ("windows", "x86_64") => Ok(("windows", "amd64")),
        (os, arch) => bail!("不支持的平台: {}-{}", os, arch),
    }
}

async fn download_url(p: Provider) -> Result<String> {
    let (os, arch) = platform()?;
    Ok(match p {
        Provider::Cloudflare => {
            let base = "https://github.com/cloudflare/cloudflared/releases/latest/download";
            match os {
                "darwin" => format!("{}/cloudflared-darwin-{}.tgz", base, arch),
                "linux" => format!("{}/cloudflared-linux-{}", base, arch),
                "windows" => format!("{}/cloudflared-windows-amd64.exe", base),
                _ => unreachable!(),
            }
        }
        Provider::Ngrok => format!(
            "https://bin.equinox.io/c/4VmDzA7iaHb/ngrok-stable-{}-{}.zip",
            os, arch
        ),
        Provider::OpenAi => {
            // 资产名带版本号,先查最新 tag
            let v: serde_json::Value = reqwest::Client::new()
                .get("https://api.github.com/repos/openai/tunnel-client/releases/latest")
                .header("User-Agent", "mcp-bridge")
                .send()
                .await?
                .json()
                .await?;
            let tag = v["tag_name"].as_str().context("未取到 tunnel-client 最新版本")?;
            format!(
                "https://github.com/openai/tunnel-client/releases/download/{}/tunnel-client-{}-{}-{}.zip",
                tag, tag, os, arch
            )
        }
    })
}

/// 下载并安装到托管目录,写 .managed 标记
pub async fn install(p: Provider, log: &LogBus) -> Result<PathBuf> {
    let dir = p.dir();
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(p.binary_name());
    let url = download_url(p).await?;
    log.emit("INFO", "隧道", format!("正在下载 {} ({})", p.display(), url));

    // 直连失败自动换镜像/代理,适配不同网络环境
    let mut bytes = None;
    let mut last_err = anyhow::anyhow!("下载未尝试");
    for (i, cand) in mirror_candidates(&url).iter().enumerate() {
        if i > 0 {
            log.emit("INFO", "隧道", format!("直连失败,换镜像: {}", cand));
        }
        for (via_proxy, client) in [
            crate::netutil::client(Duration::from_secs(90)),
            crate::netutil::proxied_client(Duration::from_secs(90)),
        ]
        .into_iter()
        .enumerate()
        {
            if via_proxy == 1 && crate::netutil::system_proxy().is_none() {
                continue;
            }
            match client.get(cand).send().await {
                Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                    Ok(b) => {
                        bytes = Some(b);
                        break;
                    }
                    Err(e) => last_err = e.into(),
                },
                Ok(resp) => last_err = anyhow::anyhow!("HTTP {}", resp.status()),
                Err(e) => last_err = e.into(),
            }
        }
        if bytes.is_some() {
            break;
        }
    }
    let bytes = bytes.context(format!("下载失败(已尝试直连/镜像/代理): {}", last_err))?;

    if url.ends_with(".zip") {
        extract_from_zip(&bytes, p.binary_name(), &dest)?;
    } else if url.ends_with(".tgz") {
        extract_from_tgz(&bytes, p.binary_name(), &dest)?;
    } else {
        tokio::fs::write(&dest, &bytes).await?;
    }
    if !dest.exists() {
        bail!("压缩包中未找到 {}", p.binary_name());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).await?;
    }
    tokio::fs::write(p.marker(), b"managed by mcp-bridge\n").await?;
    log.emit("OK", "隧道", format!("{} 已安装到 {}", p.display(), dest.display()));
    Ok(dest)
}

fn extract_from_zip(bytes: &[u8], want: &str, dest: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        if f.name().ends_with(want) {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut f, &mut out)?;
            return Ok(());
        }
    }
    bail!("zip 中未找到 {}", want)
}

fn extract_from_tgz(bytes: &[u8], want: &str, dest: &Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut ar = tar::Archive::new(gz);
    for entry in ar.entries()? {
        let mut e = entry?;
        if e.path().ok().and_then(|p| p.to_str().map(str::to_owned)).map(|p| p.ends_with(want)).unwrap_or(false) {
            e.unpack(dest)?;
            return Ok(());
        }
    }
    bail!("tgz 中未找到 {}", want)
}

/// 确保可用:系统有就用系统的,否则自动下载安装
pub async fn ensure(p: Provider, log: &LogBus) -> Result<PathBuf> {
    if let Some(path) = find(p) {
        log.emit("INFO", "隧道", format!("使用已有组件: {}", path.display()));
        return Ok(path);
    }
    install(p, log).await
}

/// 卸载我们安装的组件(带 .managed 标记才删;系统装的一概不动)
pub async fn cleanup(p: Provider, log: &LogBus) -> Result<bool> {
    if p.marker().exists() {
        tokio::fs::remove_dir_all(p.dir()).await?;
        log.emit("OK", "隧道", format!("已卸载我们安装的 {}", p.display()));
        return Ok(true);
    }
    log.emit("INFO", "隧道", format!("{} 为系统安装,不动它", p.display()));
    Ok(false)
}

pub struct TunnelHandle {
    pub provider: Provider,
    /// 公网 URL;OpenAI Tunnel 无公网入口为 None
    pub url: Option<String>,
    child: Child,
}

impl TunnelHandle {
    pub async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

/// 启动隧道
pub async fn start(p: Provider, bin: &Path, local_port: u16, creds: &Creds, log: &LogBus) -> Result<TunnelHandle> {
    match p {
        Provider::Cloudflare => start_cloudflare(bin, local_port, log).await,
        Provider::Ngrok => start_ngrok(bin, local_port, creds, log).await,
        Provider::OpenAi => start_openai(bin, local_port, creds, log).await,
    }
}

async fn start_cloudflare(bin: &Path, local_port: u16, log: &LogBus) -> Result<TunnelHandle> {
    // 先试默认 QUIC(快),失败/超时自动回退 HTTP/2(UDP 7844 被墙或公司网常见)
    let attempts: [(Vec<String>, u64); 2] = [
        (vec!["tunnel".into(), "--url".into(), format!("http://127.0.0.1:{}", local_port), "--no-autoupdate".into()], 25),
        (vec!["tunnel".into(), "--url".into(), format!("http://127.0.0.1:{}", local_port), "--no-autoupdate".into(), "--protocol".into(), "http2".into()], 40),
    ];
    let mut last_err = anyhow::anyhow!("未尝试");
    for (idx, (args, wait_secs)) in attempts.iter().enumerate() {
        if idx > 0 {
            log.emit("WARN", "隧道", "QUIC 连接失败,回退 HTTP/2 协议重试…");
        }
        let mut child = match silent(Command::new(bin).args(args))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                last_err = e.into();
                continue;
            }
        };
        if idx == 0 {
            log.emit("INFO", "隧道", "cloudflared 已启动,正在建立 quick tunnel…");
        }

        let stderr = child.stderr.take().context("无 stderr")?;
        let mut lines = BufReader::new(stderr).lines();
        let parsed = tokio::time::timeout(Duration::from_secs(*wait_secs), async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(pos) = line.find("https://") {
                    if line.contains("trycloudflare.com") {
                        let end = line[pos..]
                            .find(|c: char| c.is_whitespace() || c == '|')
                            .unwrap_or(line.len() - pos);
                        return Ok(line[pos..pos + end].to_string());
                    }
                }
            }
            Err(anyhow::anyhow!("未在日志中找到 trycloudflare URL"))
        })
        .await;

        match parsed {
            Ok(Ok(url)) => {
                // 必须持续抽干 stderr/stdout,否则管道缓冲区写满后 cloudflared 阻塞(表现为持续 530)
                let log2 = log.clone();
                tokio::spawn(async move {
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.contains("Registered tunnel connection") {
                            log2.emit("INFO", "隧道", "边缘连接已注册");
                        }
                    }
                });
                if let Some(stdout) = child.stdout.take() {
                    tokio::spawn(async move {
                        let mut l = BufReader::new(stdout).lines();
                        while let Ok(Some(_)) = l.next_line().await {}
                    });
                }
                log.emit("OK", "隧道", format!("公网地址已分配 → {}(等待边缘就绪)", url.replace("https://", "")));
                return Ok(TunnelHandle { provider: Provider::Cloudflare, url: Some(url), child });
            }
            Ok(Err(e)) => {
                last_err = e;
                let _ = child.kill().await;
            }
            Err(_) => {
                last_err = anyhow::anyhow!("等待隧道 URL 超时({}s)", wait_secs);
                let _ = child.kill().await;
            }
        }
    }
    Err(last_err).context("cloudflared 建立隧道失败")
}

async fn start_ngrok(bin: &Path, local_port: u16, creds: &Creds, log: &LogBus) -> Result<TunnelHandle> {
    let token = creds.token.clone().context("ngrok 需要 authtoken")?;
    let mut child = silent(Command::new(bin).args([
        "http",
        &format!("127.0.0.1:{}", local_port),
        "--authtoken",
        &token,
        "--log",
        "stdout",
        "--log-format",
        "json",
    ]))
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .context("ngrok 启动失败")?;

    // 抽干日志管道 + 侦测致命错误(如 authtoken 无效)
    let log2 = log.clone();
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut l = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = l.next_line().await {
                if line.contains("ERR_NGROK") {
                    log2.emit("ERR", "隧道", format!("ngrok 错误: {}", line));
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut l = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = l.next_line().await {}
        });
    }

    log.emit("INFO", "隧道", "ngrok 已启动,正在查询本地 API(127.0.0.1:4040)…");

    // 从 ngrok 本地 API 取公网地址
    let url = tokio::time::timeout(Duration::from_secs(25), async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build()?;
        loop {
            if let Ok(resp) = client.get("http://127.0.0.1:4040/api/tunnels").send().await {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    if let Some(arr) = v.get("tunnels").and_then(|t| t.as_array()) {
                        for t in arr {
                            let u = t.get("public_url").and_then(|x| x.as_str()).unwrap_or("");
                            if u.starts_with("https://") {
                                return Ok::<String, anyhow::Error>(u.to_string());
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .context("等待 ngrok 隧道超时")??;

    log.emit("OK", "隧道", format!("隧道已建立 → {}", url.replace("https://", "")));
    Ok(TunnelHandle { provider: Provider::Ngrok, url: Some(url), child })
}

async fn start_openai(bin: &Path, local_port: u16, creds: &Creds, log: &LogBus) -> Result<TunnelHandle> {
    let api_key = creds.token.clone().context("OpenAI Tunnel 需要 Platform API Key")?;
    let tunnel_id = creds.tunnel_id.clone().context("OpenAI Tunnel 需要 tunnel_id")?;

    // init 一个 profile,把上游指向本地网关
    let status = silent(Command::new(bin).args([
        "init",
        "--profile",
        "mcp-bridge",
        "--tunnel-id",
        &tunnel_id,
        "--mcp-server-url",
        &format!("http://127.0.0.1:{}/mcp", local_port),
    ]))
    .env("CONTROL_PLANE_API_KEY", &api_key)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .await
    .context("tunnel-client init 启动失败")?;
    if !status.success() {
        bail!("tunnel-client init 失败(exit {}),请检查 API Key / tunnel_id", status);
    }

    let mut child = silent(Command::new(bin).args(["run", "--profile", "mcp-bridge"]))
        .env("CONTROL_PLANE_API_KEY", &api_key)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("tunnel-client run 启动失败")?;

    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut l = BufReader::new(stdout).lines();
            while let Ok(Some(_)) = l.next_line().await {}
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut l = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = l.next_line().await {}
        });
    }

    // 进程存活 4s 视为已连接(OpenAI Tunnel 无公网 URL,走 OpenAI 平台侧鉴权)
    tokio::time::sleep(Duration::from_secs(4)).await;
    if let Ok(Some(status)) = child.try_wait() {
        bail!("tunnel-client 提前退出(exit {}),请检查 API Key / tunnel_id", status);
    }
    log.emit("OK", "隧道", "OpenAI Tunnel 已连接(无公网入口,经 OpenAI 平台转发)");
    Ok(TunnelHandle { provider: Provider::OpenAi, url: None, child })
}
