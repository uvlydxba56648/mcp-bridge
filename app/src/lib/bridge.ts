/**
 * 前后端桥接层:Tauri 桌面环境走真实 Rust 命令;纯浏览器预览(无 __TAURI_INTERNALS__)
 * 回退到 mock,保证 UI 可演示。App 只依赖本模块接口。
 */

export interface DetectInfo {
  name: string
  version: string
  endpoint: string
  transport: string
  tool_count: number
  probe_ms: number
}

export interface TunnelStatus {
  installed: boolean
  provider?: string
  url?: string | null
  gateway_port?: number | null
  tunnel_id?: string | null
}

export interface LogPayload {
  time: string
  level: "INFO" | "OK" | "WARN" | "ERR"
  source: string
  message: string
}

export interface Bridge {
  detect(): Promise<DetectInfo>
  probeManual(url: string, kind: string): Promise<DetectInfo>
  ensureTunnel(provider: string, token: string, tunnelId: string): Promise<TunnelStatus>
  cleanupComponents(): Promise<void>
  closeConnection(): Promise<void>
  hideWindow(): Promise<void>
  quitApp(): Promise<void>
  onCloseRequested(cb: () => void): () => void
  onPairing(cb: (code: string) => void): () => void
  onLog(cb: (e: LogPayload) => void): () => void
}

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window

/* ── 真实实现(Tauri) ── */
async function realBridge(): Promise<Bridge> {
  const { invoke } = await import("@tauri-apps/api/core")
  const { listen } = await import("@tauri-apps/api/event")
  return {
    detect: () => invoke<DetectInfo>("detect_mcp"),
    probeManual: (url: string, kind: string) => invoke<DetectInfo>("probe_manual", { url, kind }),
    ensureTunnel: (provider: string, token: string, tunnelId: string) =>
      invoke<TunnelStatus>("ensure_tunnel", {
        provider,
        token: token || null,
        tunnelId: tunnelId || null,
      }),
    cleanupComponents: () => invoke<void>("cleanup_components"),
    closeConnection: () => invoke<void>("close_connection"),
    hideWindow: () => invoke<void>("hide_window"),
    quitApp: () => invoke<void>("quit_app"),
    onCloseRequested: (cb) => {
      let unlisten: (() => void) | undefined
      listen("close-requested", () => cb()).then((u) => (unlisten = u))
      return () => unlisten?.()
    },
    onPairing: (cb) => {
      let unlisten: (() => void) | undefined
      listen<string>("pairing", (e) => cb(e.payload)).then((u) => (unlisten = u))
      return () => unlisten?.()
    },
    onLog: (cb) => {
      let unlisten: (() => void) | undefined
      listen<LogPayload>("log", (e) => cb(e.payload)).then((u) => (unlisten = u))
      return () => unlisten?.()
    },
  }
}

/* ── Mock 实现(浏览器预览/截图用) ── */
function now() {
  return new Date().toTimeString().slice(0, 8)
}
function mockBridge(): Bridge {
  const listeners = new Set<(e: LogPayload) => void>()
  const emit = (level: LogPayload["level"], source: string, message: string) =>
    listeners.forEach((cb) => cb({ time: now(), level, source, message }))
  return {
    detect: () =>
      new Promise((resolve) => {
        setTimeout(() => {
          emit("OK", "检测", "已连接 GoLand MCP Server 2026.1.1 @ 127.0.0.1:64342(135 ms)")
          emit("INFO", "检测", "发现 41 个工具,全部放开")
          resolve({
            name: "GoLand MCP Server",
            version: "2026.1.1",
            endpoint: "http://127.0.0.1:64342/stream",
            transport: "streamable-http",
            tool_count: 41,
            probe_ms: 135,
          })
        }, 900)
      }),
    probeManual: (url: string, kind: string) =>
      new Promise((resolve, reject) => {
        setTimeout(() => {
          try {
            const u = new URL(url)
            const endpoint = kind === "sse" ? u.origin + "/stream" : url
            emit("OK", "检测", `手动配置握手成功: GoLand MCP Server 2026.1.1(41 个工具)`)
            resolve({
              name: "GoLand MCP Server",
              version: "2026.1.1",
              endpoint,
              transport: kind === "sse" ? "sse(上游走 streamable-http)" : "streamable-http",
              tool_count: 41,
              probe_ms: 18,
            })
          } catch {
            reject(new Error("URL 不合法"))
          }
        }, 500)
      }),
    ensureTunnel: (provider: string, token: string, tunnelId: string) =>
      new Promise((resolve, reject) => {
        if (provider === "ngrok" && !token) {
          emit("ERR", "隧道", "ngrok 需要 authtoken")
          return reject(new Error("ngrok 需要 authtoken"))
        }
        if (provider === "openai" && (!token || !tunnelId)) {
          emit("ERR", "隧道", "OpenAI Tunnel 需要 Platform API Key 与 tunnel_id")
          return reject(new Error("OpenAI Tunnel 需要 Platform API Key 与 tunnel_id"))
        }
        const bin = provider === "cloudflare" ? "cloudflared" : provider === "ngrok" ? "ngrok" : "tunnel-client"
        emit("INFO", "隧道", `开始自动安装 ${bin}…`)
        setTimeout(() => {
          emit("OK", "隧道", `${bin} 安装完成`)
          setTimeout(() => {
            if (provider === "openai") {
              emit("OK", "隧道", "OpenAI Tunnel 已连接(无公网入口,经 OpenAI 平台转发)")
              resolve({ installed: true, provider, url: null, gateway_port: 50123, tunnel_id: tunnelId })
            } else {
              const host = provider === "ngrok" ? "mcpb-kite.ngrok-free.app" : "mcpb-kite-42.trycloudflare.com"
              emit("OK", "隧道", `隧道已建立 → ${host}`)
              resolve({ installed: true, provider, url: `https://${host}/mcp`, gateway_port: 50123 })
            }
          }, 1300)
        }, 1600)
      }),
    cleanupComponents: () => {
      emit("OK", "隧道", "已卸载我们安装的组件(系统自带的不动)")
      return Promise.resolve()
    },
    closeConnection: () => {
      emit("INFO", "网关", "连接已关闭,隧道已断开")
      return Promise.resolve()
    },
    hideWindow: () => Promise.resolve(),
    quitApp: () => Promise.resolve(),
    onCloseRequested: () => () => {},
    onPairing: (cb) => {
      const t = setTimeout(() => cb("K7M3-X9Q2"), 2000)
      return () => clearTimeout(t)
    },
    onLog: (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
  }
}

let cached: Bridge | null = null
export async function getBridge(): Promise<Bridge> {
  if (cached) return cached
  cached = isTauri ? await realBridge() : mockBridge()
  return cached
}
