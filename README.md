# MCP Bridge for JetBrains

把本机 JetBrains MCP Server 安全暴露给 Web ChatGPT 的三屏向导式桌面工具。

## 三屏 UI(顺序:连接 → 隧道 → 密钥)

### 第 1 屏 · 连接 IDE
启动自动检测本机 JetBrains MCP(支持 `/stream` 与 `/sse`),也可粘贴手动配置自动解析;「41 个工具」按钮弹出工具列表弹窗(搜索 + 只读/写入/执行分级标记);检测通过后进入下一步。

![第 1 屏](screenshots/screen1-connect.png)

工具列表弹窗(数据来自本机 GoLand 2026.1.1 实测):

![第 1 屏 · 工具弹窗](screenshots/screen1-tools.png)

### 第 2 屏 · 隧道穿透
默认 Cloudflare Tunnel(Quick 模式免账号),备选 ngrok / OpenAI Secure MCP Tunnel;缺少 `cloudflared` 时给出安装命令 + 「一键安装」,Named 隧道弹窗输入 API Token 并验证。

![第 2 屏](screenshots/screen2-tunnel.png)

### 第 3 屏 · 生成密钥
生成访问鉴权 API Key(可复制 / 生成新 Key),展示公网地址,一键复制 ChatGPT 配置;🟢 绿点 = 端到端自检通过,可直接去 ChatGPT(Settings → Apps → Create)配置;底部为格式化日志窗口(时间/级别/来源/消息分列),右下「关闭连接」可一键断开。

![第 3 屏](screenshots/screen3-ready.png)

## 应用实现(app/ 目录,已可跑)

基于 **Tauri 2 + Vite + React 19 + TS + Tailwind v4 + shadcn/ui(Radix)** 的工程已实现三屏向导(含状态流转、模拟检测/安装/建隧道、Key 生成、剪贴板、toast、日志流),实跑截图:

| S1 连接 | S1 工具弹窗 | S2 隧道(一键安装后) | S3 就绪+日志 |
|---|---|---|---|
| ![s1](screenshots/app-s1-connect.png) | ![s1t](screenshots/app-s1-tools.png) | ![s2](screenshots/app-s2-tunnel.png) | ![s3](screenshots/app-s3-ready.png) |

```bash
cd app && pnpm install && pnpm dev      # 浏览器预览(mock 数据)
pnpm tauri dev                          # 桌面窗口(macOS 需 Xcode CLT;Linux 需 webkit2gtk-4.1)

cd app/bridge-core && cargo test -- --ignored   # 对本机真实 IDE 的集成测试
cargo run --example e2e                          # 端到端真链路:探测→网关→CF 隧道→公网自检
```

**真链路已实现并实测通过**:Rust 核心 `bridge-core`(MCP 并行探测/常驻上游会话/tools 缓存、axum 鉴权网关、cloudflared 托管),前端经 `src/lib/bridge.ts` 走 Tauri invoke + 日志事件;默认 41 个工具全部放开。实测:探测 ~56ms、缓存 tools/list 2ms、经公网 tools/call ~0.7s。

组件全部来自 shadcn/ui(源码复制进 `src/components/ui/`,可直接改)：`button / input / dialog / badge / radio-group / toggle-group / scroll-area / separator / sonner(toast)`,图标 `lucide-react`。工具数据 `src/data/tools.json` 为本机 GoLand 2026.1.1 实测的 41 个工具;风险分级与白名单在 `src/lib/toolmeta.ts`。

## 目录

- `docs/DESIGN.md` — 架构、技术栈选型(Tauri + shadcn/ui)、MCP 传输性能分析(SSE vs Streamable HTTP + 隧道开销)、JetBrains 40+ 工具盘点与「原样转发」风险分析、安全清单
- `mockups/*.html` — 高保真 UI 原型(可作为实现稿)
- `app/` — 可运行工程(Tauri + React + shadcn/ui)
- `tools/probe.js` — 本机 MCP 探测脚本(连 127.0.0.1:64342 抓取真实工具清单)
- `screenshots/*.png` — 1760×1200@2x 渲染图

## 技术栈速览

Tauri 2 (Rust, 冷启动 <1s / 包 ~10MB) + Vite + React + shadcn/ui;内嵌 axum 鉴权网关(只绑 127.0.0.1,工具白名单、输出截断、Bearer Key 鉴权),cloudflared 以 sidecar 托管。
