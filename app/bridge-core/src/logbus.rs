use serde::Serialize;
use tokio::sync::broadcast;

/// 一条结构化日志(对应前端日志窗口的四列)
#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub time: String,
    pub level: String, // INFO | OK | WARN | ERR
    pub source: String, // 检测 | 隧道 | 网关 | 自检
    pub message: String,
}

impl LogEvent {
    pub fn new(level: &str, source: &str, message: impl Into<String>) -> Self {
        // HH:MM:SS,本地时区
        let time = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // 简单 UTC+8 格式足够(桌面工具);需要精确时区可换 time crate
            let t = (now + 8 * 3600) % 86400;
            format!("{:02}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
        };
        Self {
            time,
            level: level.into(),
            source: source.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone)]
pub struct LogBus {
    tx: broadcast::Sender<LogEvent>,
}

impl LogBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn emit(&self, level: &str, source: &str, message: impl Into<String>) {
        let _ = self.tx.send(LogEvent::new(level, source, message));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.tx.subscribe()
    }
}

impl Default for LogBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 配对码事件总线:网关产生配对码 → 桌面弹窗展示
#[derive(Clone)]
pub struct PairBus {
    tx: broadcast::Sender<String>,
}

impl PairBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self { tx }
    }
    pub fn emit(&self, code: String) {
        let _ = self.tx.send(code);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

impl Default for PairBus {
    fn default() -> Self {
        Self::new()
    }
}
