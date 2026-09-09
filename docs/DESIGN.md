# MCP Bridge for JetBrains — 设计 & 分析文档

把本机 JetBrains IDE 的 MCP Server(`127.0.0.1:64342`,无鉴权)安全地暴露给 Web 版 ChatGPT 的桌面小工具。

## 0. 整体架构

```
┌─────────────┐   HTTPS    ┌──────────────┐   QUIC/HTTP2   ┌──────────────────┐  localhost   ┌────────────────┐
│ Web ChatGPT │ ────────►  │ Cloudflare   │ ─────────────► │ MCP Bridge 网关  │ ──────────►  │ JetBrains MCP  │
│ (自定义 App) │ ◄──────── │ Tunnel 边缘  │ ◄───────────── │ 鉴权+过滤+适配   │ ◄──────────  │ 127.0.0.1:64342│
└─────────────┘            └──────────────┘                └──────────────────┘              └────────────────┘
                                                                  │ 只监听 127.0.0.1
                                                                  └─ 托管 cloudflared 子进程
```

**核心原则:绝不能把裸的 JetBrains MCP 直接挂隧道。** 它没有鉴权,谁拿到 URL 谁就能 `execute_terminal_command` —— 等于把电脑终端开放给整个互联网。MCP Bridge 网关是鉴权与过滤的唯一入口。

## 1. 三屏交互流程

| 屏 | 目标 | 关键元素 | 通过条件 |
|---|---|---|---|
| **S1 连接 IDE** | 找到本机 MCP | 自动检测卡片(端口/IDE 版本/工具数) + 工具列表弹窗(搜索 + 只读/写入/执行分级) + 手动粘贴 JSON 解析 | `tools/list` 探测成功,「下一步」解锁 |
| **S2 隧道穿透** | 公网可达 | 方案单选列表(Cloudflare Tunnel 默认 / ngrok / OpenAI Tunnel);未安装组件时显示安装命令 + 「一键安装」;Token 输入 + 验证 | 隧道连通性自检通过 |
| **S3 生成密钥** | 交付配置 | 🟢 绿点(端到端自检通过)、公网 URL、API Key(复制 / 生成新 Key)、格式化日志窗口(时间/级别/来源/消息)、「复制 ChatGPT 配置」「关闭连接」 | 通过公网 URL 回环调用成功 |

UI 图:`../screenshots/screen1-connect.png`、`screen1-tools.png`(工具弹窗)、`screen2-tunnel.png`、`screen3-ready.png`(HTML 高保真原型见 `../mockups/`,可直接作为实现稿)。

### S1 细节:自动检测逻辑
- 扫描 JetBrains 内置 Web Server 常见端口 `63342`(默认)与 `64342` 等,探测 `/stream`(Streamable HTTP)与 `/sse`(SSE)两个端点;
- 发起 MCP `initialize` + `tools/list` 握手,成功即显示工具数量;
- 手动粘贴:兼容用户从 IDE「Manual Client Configuration」复制的 `{"type":"streamable-http","url":...}` / `{"type":"sse","url":...}` JSON,自动解析 url/type。

### S2 细节:隧道方案
| 方案 | 依赖 | 需要 Key? | 说明 |
|---|---|---|---|
| **Cloudflare Tunnel(默认)** | `cloudflared`(软件自动下载官方 release) | Quick 模式**不需要**;Named 隧道需要 API Token(弹窗输入) | 免费、自带 HTTPS、国内可访问性相对好 |
| ngrok | `ngrok` agent | 需要 authtoken | 免费版域名每次变化 |
| OpenAI Secure MCP Tunnel | 官方 `tunnel-client` | 需要 Platform API Key + tunnel_id | **不产生公网入口**,出站长轮询;但需要 ChatGPT 工作区/Platform 组织权限,适合企业 |

「自动安装」:检测组件时**系统 PATH 优先**(`which/where`);缺失才从官方源自动下载到托管目录 `~/.local/share/mcp-bridge/bin/<provider>/` 并写 `.managed` 标记;标题栏「⋯ → 卸载已安装的组件」只清带标记的,系统装的一概不动。

三种方案的实现状态:
- **Cloudflare**(默认):quick tunnel,已在本机端到端实测(含公网回环自检);
- **ngrok**:`ngrok http --authtoken`,公网地址从本地 API `127.0.0.1:4040/api/tunnels` 轮询获取;凭 authtoken,待真实凭据实测;
- **OpenAI Tunnel**:`tunnel-client init + run`(自动解析 GitHub 最新 release 的资产名);**无公网入口**,此模式下网关切为免 Key(鉴权在 OpenAI 平台侧),S3 展示 Tunnel ID 供 ChatGPT「Connection → Tunnel」粘贴;待真实凭据实测。

### S3 细节:密钥与配置(OAuth 2.1 模式)
- 界面只保留一个 **URL 复制框**(纯 `https://<域名>/mcp`,不含任何密钥);身份验证在 ChatGPT 侧选 **OAuth**,浏览器会弹本软件内嵌授权页,点「允许」即完成;
- 绿点含义:经公网 URL 回环调用成功(自检 token 服务端内部铸造,不落界面);
- 鉴权兼容:OpenAI Tunnel 模式免鉴权(无公网入口,鉴权在 OpenAI 平台侧)。
- 鉴权兼容:ChatGPT 自定义连接器主要支持 None / OAuth。网关因此**同时接受三种携带方式**:`Authorization: Bearer <key>` 头、URL 路径嵌入 `https://<域名>/k/<key>/mcp`(供选择"无认证"时使用)、以及 v2 版本的内建 OAuth 2.1 端点。

## 2. 前端技术栈(启动 ≤1s)

| 候选 | 冷启动 | 包体积 | 内存 | 结论 |
|---|---|---|---|---|
| **Tauri 2 + Rust** | **~0.3–0.8s** | **~10MB** | 低 | ✅ 采用。系统 WebView 免打包 Chromium,Rust 后端天然胜任网关+子进程托管 |
| Electron | 2–5s | 150MB+ | 高 | ❌ 违反 1s 启动要求 |
| Wails(Go) | ~1s | ~15MB | 低 | 备选(若团队偏 Go) |
| 纯 Web + 本地 agent | — | — | — | ❌ 浏览器无法访问 127.0.0.1 裸 HTTP(混合内容/PNA 限制),必须桌面端 |

- **UI 规范**:遵循 macOS HIG——亮色窗口(#F5F5F7 底 + 白色分组卡片)、左上角红黄绿交通灯、系统蓝 #007AFF 默认按钮(右下:取消/继续)、白底细边框文本框 + 聚焦蓝环、13px 基准字号、发丝分割线(#E2E2E6)、无渐变无发光;控件原型见 `../mockups/`。
- **UI 组件不手写**:shadcn/ui(Radix Primitives + Tailwind CSS,GitHub ~80k★)——Wizard 步骤条、Radio 卡片、Input、Button、Toast 全部现成;若追求更贴近 macOS 原生观感,可用 **Park UI**(Ark UI 生态,带 macOS 风格 preset)或直接以 `mockups/` 中的 HIG 原型为准实现;替代品:Ark UI、Naive UI(Vue)。
- 构建:Vite + React 19;前端产物是纯静态文件,Tauri 直接内嵌,启动无打包器开销。
- **视觉(Liquid Glass 主题,2026-09 替换原纯 HIG 皮肤)**:整窗背景为原生 WebGL1 折射渲染(域扭曲 fbm 极光 + RGB 色散采样 + 边缘高光棱线,`app/src/aurora.ts`,零依赖 ~4KB,0.66x 降采样,页面隐藏/reduced-motion 自动停帧,WebGL 缺失降级静态渐变);前景卡片 `backdrop-filter: blur+saturate` 玻璃拟态,由 index.css 尾部非层级 CSS 覆盖 Tailwind 工具类实现,组件 JSX 零改动。网页授权页内嵌同一 shader 的无模块版(`bridge-core/src/aurora_web.js`,`include_str!` 打包进二进制)。
- 网关:Rust `axum` 内嵌于 Tauri 后端,反向代理到 `127.0.0.1:64342`;`cloudflared` 作为 sidecar 子进程拉起并解析日志中的临时域名。

## 3. MCP 网络传输性能分析

### 3.1 SSE vs Streamable HTTP(JetBrains 两个都暴露)

| | SSE 传输(旧,spec 2024-11-05) | Streamable HTTP(spec 2025-03-26+) |
|---|---|---|
| 端点 | **双端点**:GET `/sse` 长连接收事件 + POST `/messages` 发请求 | **单端点** `/stream`:POST 即请求;需要流式时服务器把该响应升级为 SSE |
| 连接占用 | 每个客户端 2 条 TCP,长连接在隧道/代理下易被空闲超时掐断 | 普通请求-响应为主,更贴合 CDN/隧道;可选 SSE 流 |
| 断线恢复 | 依赖可选的 `Last-Event-ID`,实现参差 | 内建事件 ID + `Mcp-Session-Id`,支持恢复 |
| 状态 | 已被官方标记 **deprecated** | 现行标准 |

**结论:网关上游连 JetBrains 优先选 `/stream`;对外暴露统一为 `/mcp`(Streamable HTTP),仅为了兼容旧客户端保留 `/sse` 透传。** ChatGPT 与 Responses API 两种都支持。

### 3.2 经过隧道后的额外开销

1. **延迟**:一次工具调用 = ChatGPT 后端(美国) → CF 边缘 → `cloudflared` → 本机 IDE → 原路返回。链路增加约 **150–600ms**(视本地到 CF 节点质量),叠加 IDE 自身执行耗时。`build_project`、`lint_files` 这类秒级工具调用,体感瓶颈在 IDE 而非网络。
2. **SSE 缓冲坑**:Cloudflare 对 `text/event-stream` 不做缓冲,OK;但如果以后换别的反代,必须确认不缓冲 SSE(必要时加 `X-Accel-Buffering: no`),否则流式事件会被攒包。
3. **Quick Tunnel 无 SLA**:`*.trycloudflare.com` 域名随机、会漂移;要固定域名就得上 Named Tunnel(需要 Token,S2 弹窗输入的就是它)。
4. **大输出放大成本**:`get_file_text_by_path`、终端输出动辄几十 KB → 经隧道回传 + 进模型上下文,**延迟与 token 双爆炸**。网关必须默认截断(如 8KB)+ 提示分页。

### 3.3 网关侧优化
- 本地缓存 `tools/list` 结果(ChatGPT 每会话只拉一次并冻结快照,网关返回缓存可同时降低延迟与保证快照一致);
- 默认只读白名单把 42 个工具砍到 ~12 个(OpenAI 官方文档明确建议:工具过多 → 高成本 + 高延迟,用 `allowed_tools` 过滤);
- 每个请求强制超时(默认 30s)与输出上限。

## 4. JetBrains MCP 工具盘点 & 原样转发的后果

> 以下数据来自**实测**:直接连本机 `http://127.0.0.1:64342/stream`(Streamable HTTP 握手成功)抓取,完整 dump 见 `jetbrains-mcp-tools.json`(探测脚本 `tools/probe.js`,任何机器跑一遍即可拿到自己的真实清单)。

### 4.1 实测结果(GoLand 2026.1.1,41 个工具)

- **Server 标识**:`GoLand MCP Server 2026.1.1`;capabilities 仅 `tools.listChanged`,无 resources/prompts
- **工具定义总体积**:**48,363 字符 ≈ 1.4 万–2.4 万 tokens** —— 这就是"原样转发"时每次会话要注入上下文的固定成本
- 最大的几个:`execute_run_configuration`(3.4KB)、`read_file`(2.9KB)、`replace_text_in_file`(2.1KB)、`execute_terminal_command`(2.1KB)

按风险分级(实测 41 个):

| 级别 | 数量 | 工具 |
|---|---|---|
| 🟢 只读 | 30 | `read_file`、`get_file_text_by_path`、`find_files_by_glob`、`find_files_by_name_keyword`、`list_directory_tree`、`get_all_open_file_paths`、`search_in_files_by_text/regex`、`search_text`/`search_regex`/`search_file`/`search_symbol`(新旧两代搜索并存!)、`get_symbol_info`、`get_file_problems`、`get_project_modules`、`get_project_dependencies`、`get_run_configurations`、`get_repositories`、`generate_psi_tree`、`generate_inspection_kts_api/examples`、数据库只读 ×8(`list_*`、`preview_table_data`、`test_database_connection`、`get_database_object_description`)、`cancel_sql_query` |
| 🔴 写/执行 | 11 | **`execute_terminal_command`(= 任意 shell,RCE)**、`runNotebookCell`(执行 Jupyter 单元格 = 代码执行)、`run_inspection_kts`(在 IDE 内执行 Kotlin 脚本)、`replace_text_in_file`、`create_new_file`、`rename_refactoring`、`reformat_file`、`execute_run_configuration`、`build_project`、`open_file_in_editor`、`execute_sql_query`(可写库) |

**与官方文档的差异(实测修正)**:
- 文档里列的 `xdebug_*` 调试器全家桶在此版本**并未暴露**;
- 出现了文档没有的 AI 向工具(`generate_psi_tree`、`run_inspection_kts`);
- 新旧两代搜索工具并存(`search_in_files_by_regex` vs `search_regex` 等),说明工具集处于快速变动期 —— **软件必须在 S1 实测抓取,绝不能硬编码工具清单**。

### 4.2 如果原样转发给 Web ChatGPT,会发生什么

1. **安全事故**:无鉴权 + 公网 URL = 任何人都能 `execute_terminal_command` / `runNotebookCell`,直接 RCE 开发机。**这是本软件存在的核心理由。**
2. **上下文爆炸**:实测 **48KB ≈ 1.4万–2.4万 tokens** 的工具定义全部注入上下文,每次会话都烧;OpenAI 官方文档明确警告工具过多 → 高成本 + 高延迟,建议 `allowed_tools` 过滤。
3. **写操作体验差**:ChatGPT 对写/改动作会反复弹确认,高风险动作可能被直接 block。
4. **路径歧义**:几乎所有工具都要 `projectPath`(本机绝对路径),ChatGPT 不知道你的项目在哪 → 调用失败或误操作。网关应在上游注入默认 `projectPath`。
5. **快照漂移**:ChatGPT 发布 App 后冻结工具快照;JetBrains 工具集快速变动(实测已见新旧并存)→ 网关固定白名单 + 稳定命名,可缓冲上游变化。

**默认白名单建议(12 个,只读)**:`read_file`、`list_directory_tree`、`find_files_by_glob`、`search_regex`、`search_text`、`search_symbol`、`get_symbol_info`、`get_file_problems`、`get_project_modules`、`get_project_dependencies`、`get_all_open_file_paths`、`get_run_configurations`。11 个 🔴 写/执行工具默认关闭,UI 中显式开启并二次确认。

## 5. ChatGPT 侧落地步骤(依据官方文档)

1. 旧版 Plugins 已下线;个人/开发者场景走 **Developer Mode 自定义 App**(Business/Enterprise/Edu 全功能;Pro 仅 read/fetch);
2. `Settings → Apps → Create` → 表单只有:名称、**MCP Server URL**、**身份验证(仅「无身份验证」或 OAuth,没有 API Key 头选项)** → Scan Tools → Create;
3. 因此本软件默认用**无身份验证 + URL 路径内嵌密钥**(`https://<隧道域名>/k/<api-key>/mcp`),S3 的复制框直接给出这个完整 URL;OAuth 2.1 端点为 v2 计划;
4. 「公开发布」走的是另一条重流程(plugin submission:组织身份验证 + 域名验证 `/.well-known/openai-apps-challenge` + 工具注解审核),个人本机工具不需要;
5. 快照漂移注意:发布后工具集冻结,变更需在后台 Refresh。

**更稳的官方替代**:OpenAI **Secure MCP Tunnel**(`openai/tunnel-client`)——出站长轮询、零公网暴露,但需要 Platform 组织 tunnel 权限,适合企业;个人快速玩还是 CF Tunnel 最快,所以 S2 默认 CF。

## 7. 真链路实现与实测(已完成)

### 代码结构

```
app/
├── bridge-core/          # 纯 Rust 核心(无 GUI 依赖,可独立测试)
│   ├── src/mcp.rs        # 并行探测(64342/63342 × /stream|/sse)、常驻上游会话、tools/list 缓存
│   ├── src/gateway.rs    # axum 鉴权网关(127.0.0.1):Bearer/路径嵌入 Key、initialize 本地应答、输出截断 8KB
│   ├── src/tunnel.rs     # cloudflared 查找/自动下载/quick tunnel/URL 解析(持续抽干 stderr 防管道阻塞)
│   ├── src/logbus.rs     # 结构化日志总线 → 前端日志窗口
│   ├── tests/live.rs     # 对本机 GoLand 的集成测试(401/缓存命中/真实调用)
│   └── examples/e2e.rs   # 端到端:探测→预热→网关→隧道→公网自检
└── src-tauri/            # 薄命令层:detect_mcp / ensure_tunnel / regenerate_key / close_connection + 日志事件推送
```

### 延迟优化措施(对应“减少延迟感”)

1. **启动即预热**:`detect_mcp` 成功后立刻建立上游 MCP 会话、拉取 tools/list 入缓存、启动网关——用户点“继续”时一切就绪;
2. **并行端口探测**:4 个端点同时握手,首个成功即返回(实测 56–135 ms);
3. **initialize 本地应答**、tools/list 缓存命中:**网关实测 2–3 ms**(零上游往返);
4. **上游会话复用 + 连接池**(keep-alive):tools/call 本地段仅 **5 ms**;会话 404 过期自动重建重试;
5. **projectPath 自动注入**:上游报 “Unable to determine the target project” 时,网关注入配置的默认项目路径——实测真机确实会踩到这个坑;
6. **输出截断 8KB**:防整文件/终端缓冲烧 token 与拖慢链路。

### 实测端到端数据(本机 GoLand 2026.1.1 + quick tunnel)

| 环节 | 耗时 |
|---|---|
| 探测(4 端点并行) | 56–135 ms |
| 预热 tools/list | 37–84 ms |
| 网关本地 tools/list(缓存) | **2–3 ms** |
| 网关本地 tools/call | **5 ms** |
| 经公网 tools/list(ChatGPT 每会话一次) | ~1.4 s |
| 经公网 tools/call | ~0.7 s |

### 踩坑记录(真机验证)

- **JetBrains 内置 Web Server 拒绝非本机 Host**(403)——不能直接拿隧道怼 64342,必须经过网关转发;
- **cloudflared stderr 必须持续抽干**:找到 URL 就停读会让 64KB 管道缓冲区写满、进程阻塞,边缘永远 530;
- **新 trycloudflare 域名 DNS 传播需数秒**:自检必须退避重试(实现为 10 次 × 4s);
- 本机若网络对 TLS 指纹敏感,rustls 直连可能被切断,`native-tls` 更稳(已切换)。

### 桌面端构建

macOS:`pnpm tauri dev / build`(Xcode CLT);Linux:需 `webkit2gtk-4.1` 等系统依赖。bridge-core 全部逻辑均有无 GUI 的集成测试覆盖(`cargo test -- --ignored`)。

## 8. OAuth 2.1 授权服务器(内嵌,bridge-core/src/oauth.rs)

按 OpenAI plugins/build/auth 与 MCP 授权规范实现,已通过全流程集成测试(DCR → 授权页 → PKCE 换 token → /mcp → refresh 轮换):

- `GET /.well-known/oauth-protected-resource`(RFC 9728)+ 401 带 `WWW-Authenticate: Bearer resource_metadata=...`;
- `GET /.well-known/oauth-authorization-server`(RFC 8414):含 `code_challenge_methods_supported: ["S256"]`(硬性要求)、`authorization_response_iss_parameter_supported: true`;
- `POST /oauth/register`:DCR(RFC 7591),ChatGPT 每连接注册一次;
- `GET/POST /oauth/authorize`:中文 consent 页(允许/拒绝),授权响应带 `iss`(RFC 9207);
- `POST /oauth/token`:授权码 + PKCE S256 校验换 token;access 1h,refresh 30d **每次刷新轮换**(public client MUST);
- resource(RFC 8707)透传并绑定 aud(校验忽略 http/https scheme 差异,兼容本地回环);
- 内存存储:重启后令牌失效,ChatGPT 收 401 自动重走授权。

公开插件发布(域名验证/审核/身份验证)是 Marketplace 流程,本机工具不需要。

## 6. 安全清单

- [x] 网关只绑 `127.0.0.1`,公网唯一入口是隧道
- [x] 所有外部请求强制 `Bearer Key` 校验,错误统一 401 不泄露细节
- [x] Key 存系统钥匙串;支持一键重置(旧 Key 立即失效)
- [x] 默认~~只读白名单~~ **全部工具放开**(用户确认;执行类工具风险已在弹窗中标记)
- [x] 全量请求日志本地留存(谁、何时、调了什么、IDE 返回大小)
- [x] Quick Tunnel 域名即密码强度的随机串,但仍依赖 Key 鉴权兜底
