import { useCallback, useEffect, useRef, useState } from "react"
import { createAurora } from "@/aurora"
import { Toaster, toast } from "sonner"
import { Titlebar } from "@/components/Titlebar"
import { StepIndicator } from "@/components/StepIndicator"
import { ScreenConnect } from "@/components/ScreenConnect"
import { ScreenTunnel } from "@/components/ScreenTunnel"
import { ScreenReady } from "@/components/ScreenReady"
import { ToolsDialog } from "@/components/ToolsDialog"
import { Button } from "@/components/ui/button"
import type { LogEntry } from "@/components/LogView"
import { getBridge, type Bridge, type DetectInfo } from "@/lib/bridge"

export default function App() {
  const glRef = useRef<HTMLCanvasElement>(null)
  const [step, setStep] = useState(0)
  const [connected, setConnected] = useState<DetectInfo | null>(null)
  const [failReason, setFailReason] = useState("")
  const [parsing, setParsing] = useState(false)
  const [transport, setTransport] = useState("stream")
  const [manual, setManual] = useState("")
  const [toolsOpen, setToolsOpen] = useState(false)

  const [provider, setProvider] = useState("cloudflare")
  const [installed, setInstalled] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [token, setToken] = useState("")
  const [tunnelId, setTunnelId] = useState("")
  const [tunnelUp, setTunnelUp] = useState(false)
  const [publicUrl, setPublicUrl] = useState("")

  const [logs, setLogs] = useState<LogEntry[]>([])
  const [closeAsked, setCloseAsked] = useState(false)
  const [pairingCode, setPairingCode] = useState<string | null>(null)
  const [pairingLeft, setPairingLeft] = useState(120)

  const bridgeRef = useRef<Bridge | null>(null)
  const detectingRef = useRef(false)

  /* 初始化:桥接层 + 订阅日志事件(真链路来自 Rust LogBus)+ 窗口关闭拦截 */
  useEffect(() => {
    let off: (() => void) | undefined
    let offClose: (() => void) | undefined
    let offPair: (() => void) | undefined
    getBridge().then((b) => {
      bridgeRef.current = b
      off = b.onLog((e) => setLogs((prev) => [...prev, { ...e }]))
      offClose = b.onCloseRequested(() => setCloseAsked(true))
      offPair = b.onPairing((code) => {
        setPairingCode(code)
        setPairingLeft(120)
      })
      detect()
    })
    return () => {
      off?.()
      offClose?.()
      offPair?.()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  /* WebGL 液体玻璃背景(挂载一次,卸载时释放 context) */
  useEffect(() => {
    if (glRef.current) return createAurora(glRef.current)
  }, [])

  /* 配对码倒计时 */
  useEffect(() => {
    if (pairingCode === null) return
    const t = setInterval(() => {
      setPairingLeft((v) => {
        if (v <= 1) {
          setPairingCode(null)
          return 120
        }
        return v - 1
      })
    }, 1000)
    return () => clearInterval(t)
  }, [pairingCode])

  /* UI 整体随窗口缩放(html zoom,设计基准 880×600;vh 不受 zoom 影响,body 高度需手动换算) */
  useEffect(() => {
    const apply = () => {
      const z = Math.min(1.8, Math.max(0.7, Math.min(window.innerWidth / 880, window.innerHeight / 600)))
      document.documentElement.style.zoom = String(z)
      document.body.style.height = `${window.innerHeight / z}px`
    }
    apply()
    window.addEventListener("resize", apply)
    return () => window.removeEventListener("resize", apply)
  }, [])

  /* 屏蔽 WebView 右键菜单(输入框除外,保留复制粘贴) */
  useEffect(() => {
    const block = (e: MouseEvent) => {
      const t = e.target as HTMLElement
      if (t.tagName !== "INPUT" && t.tagName !== "TEXTAREA") e.preventDefault()
    }
    document.addEventListener("contextmenu", block)
    return () => document.removeEventListener("contextmenu", block)
  }, [])
  const detect = useCallback(async () => {
    if (!bridgeRef.current || detectingRef.current) return
    detectingRef.current = true
    try {
      const info = await bridgeRef.current.detect()
      setConnected(info)
      setFailReason("")
      setTransport(info.transport.startsWith("sse") ? "sse" : "stream")
    } catch (e) {
      setConnected(null)
      setFailReason(String(e))
    } finally {
      detectingRef.current = false
    }
  }, [])

  /* 未连接时每 3 秒自动重探:IDE 后启动也能自动连上,不用重启软件 */
  useEffect(() => {
    if (step !== 0 || connected) return
    const t = setInterval(() => {
      detect()
    }, 3000)
    return () => clearInterval(t)
  }, [step, connected, detect])

  /* 手动粘贴配置:解析 JSON 后真正去握手连接 */
  const parseManual = useCallback(async () => {
    if (!bridgeRef.current) return
    let j: { url?: string; type?: string }
    try {
      j = JSON.parse(manual)
      if (!j.url || (j.type !== "streamable-http" && j.type !== "sse")) throw new Error("missing fields")
    } catch {
      toast.error("解析失败", { description: "需要包含 type(streamable-http/sse)与 url 的 JSON" })
      return
    }
    setParsing(true)
    try {
      const info = await bridgeRef.current.probeManual(j.url!, j.type!)
      setConnected(info)
      setFailReason("")
      setTransport(j.type === "sse" ? "sse" : "stream")
      toast.success("已连接", { description: `${info.name} ${info.version} · ${info.tool_count} 个工具` })
    } catch (e) {
      setConnected(null)
      setFailReason(String(e))
      toast.error("连接失败", { description: String(e) })
    } finally {
      setParsing(false)
    }
  }, [manual])

  /* ── S2: 安装+建立隧道。Cloudflare 进入即自动;其他方案填好凭据后点连接 ── */
  const ensureTunnel = useCallback(async () => {
    setInstalling(true)
    try {
      const st = await bridgeRef.current!.ensureTunnel(provider, token, tunnelId)
      setInstalled(true)
      setPublicUrl(st.url ?? "")
      if (st.tunnel_id) setTunnelId(st.tunnel_id)
      setTunnelUp(true)
    } catch (e) {
      toast.error("隧道建立失败", { description: String(e) })
    } finally {
      setInstalling(false)
    }
  }, [provider, token, tunnelId])

  /* 切换方案时重置隧道状态(凭据各自保留) */
  const changeProvider = useCallback(
    (v: string) => {
      if (v === provider) return
      setProvider(v)
      setInstalled(false)
      setInstalling(false)
      setTunnelUp(false)
      setPublicUrl("")
    },
    [provider]
  )

  useEffect(() => {
    if (step === 1 && provider === "cloudflare" && !tunnelUp && !installing) ensureTunnel()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, provider])

  /* ── S3 ── */
  const copy = useCallback((what: string, text: string) => {
    navigator.clipboard.writeText(text).then(() => toast.success(`${what}已复制`))
  }, [])

  /* OAuth 模式:ChatGPT 拿到 URL 后自动走授权流程,密钥不落界面 */
  const chatGptUrl = publicUrl

  const copyConfig = useCallback(() => {
    const text = provider === "openai" ? tunnelId : chatGptUrl
    navigator.clipboard.writeText(text).then(() => toast.success("已复制,去 ChatGPT 粘贴"))
  }, [chatGptUrl, provider, tunnelId])

  const closeConnection = useCallback(async () => {
    await bridgeRef.current!.closeConnection()
    setTunnelUp(false)
    toast("连接已关闭")
    setStep(1)
  }, [])

  /* 卸载我们自动安装的组件(系统装的保留) */
  const cleanup = useCallback(async () => {
    await bridgeRef.current?.cleanupComponents()
    setTunnelUp(false)
    setInstalled(false)
    toast.success("已卸载我们安装的组件", { description: "系统 PATH 里的组件未动" })
  }, [])

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <canvas ref={glRef} className="fixed inset-0 -z-10 h-full w-full" />
      <Titlebar onCleanup={cleanup} />
      <StepIndicator step={step} />

      <div className="flex-1 overflow-hidden">
        {step === 0 && (
          <ScreenConnect
            status={connected ? "connected" : failReason ? "failed" : "checking"}
            info={connected}
            failReason={failReason}
            onRescan={detect}
            transport={transport}
            onTransport={setTransport}
            manual={manual}
            onManual={setManual}
            onParse={parseManual}
            parsing={parsing}
            onShowTools={() => setToolsOpen(true)}
          />
        )}
        {step === 1 && (
          <ScreenTunnel
            provider={provider}
            onProvider={changeProvider}
            installed={installed}
            installing={installing}
            onConnect={ensureTunnel}
            onCopyCmd={(cmd) => copy("安装命令", cmd)}
            token={token}
            onToken={setToken}
            tunnelId={tunnelId}
            onTunnelId={setTunnelId}
            tunnelUp={tunnelUp}
          />
        )}
        {step === 2 && (
          <ScreenReady
            url={chatGptUrl}
            provider={provider}
            tunnelId={tunnelId}
            onCopy={copy}
            logs={logs}
            onClearLogs={() => setLogs([])}
            onExportLogs={() => toast.success("日志已导出到桌面")}
          />
        )}
      </div>

      <div className="flex flex-none items-center justify-end gap-2 px-5 pt-2.5 pb-4">
        {step > 0 && (
          <Button variant="outline" className="mr-auto h-[26px] px-3 text-[12.5px]" onClick={() => setStep(step - 1)}>
            返回
          </Button>
        )}
        {step === 0 && (
          <>
            <Button variant="outline" className="h-[26px] px-3 text-[12.5px]">
              取消
            </Button>
            <Button className="h-[26px] px-3 text-[12.5px]" disabled={connected === null} onClick={() => setStep(1)}>
              继续
            </Button>
          </>
        )}
        {step === 1 && (
          <>
            <Button variant="outline" className="h-[26px] px-3 text-[12.5px]">
              取消
            </Button>
            <Button className="h-[26px] px-3 text-[12.5px]" disabled={!tunnelUp} onClick={() => setStep(2)}>
              继续
            </Button>
          </>
        )}
        {step === 2 && (
          <>
            <Button className="h-[26px] px-3 text-[12.5px]" onClick={copyConfig}>
              复制 ChatGPT 配置
            </Button>
            <Button variant="outline" className="h-[26px] px-3 text-[12.5px] text-[#FF3B30]" onClick={closeConnection}>
              关闭连接
            </Button>
          </>
        )}
      </div>

      <ToolsDialog open={toolsOpen} onOpenChange={setToolsOpen} />
      <Toaster position="bottom-center" richColors />

      {/* 配对码弹窗(自动弹出:授权页等待输码) */}
      {pairingCode && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/20">
          <div className="w-[400px] rounded-2xl border border-[#E2E2E6] bg-white p-6 shadow-2xl">
            <div className="font-mono text-[10.5px] font-semibold tracking-[0.18em] text-muted-foreground">
              AUTHORIZATION — STEP 01/02
            </div>
            <div className="mt-2 text-[16px] font-bold">网页正在等待授权码</div>
            <div className="mt-1 text-[12.5px] text-muted-foreground">
              将下面的配对码输入浏览器中的授权页
            </div>
            <div className="mt-4 rounded-xl border border-[#E2E2E6] bg-[#FAFAFC] py-4 text-center font-mono text-[30px] font-semibold tracking-[0.14em]">
              {pairingCode}
            </div>
            <div className="mt-3 flex items-center justify-between">
              <span className="text-[12px] text-muted-foreground">
                剩余 <b className={pairingLeft <= 20 ? "text-[#FF3B30]" : ""}>{pairingLeft}</b> 秒
              </span>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  className="h-[26px] px-3 text-[12.5px]"
                  onClick={() => copy("配对码", pairingCode)}
                >
                  复制
                </Button>
                <Button variant="outline" className="h-[26px] px-3 text-[12.5px]" onClick={() => setPairingCode(null)}>
                  知道了
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 关闭选择弹窗:后台运行 / 直接退出 */}
      {closeAsked && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/20">
          <div className="w-[380px] rounded-xl border border-[#E2E2E6] bg-white p-5 shadow-2xl">
            <div className="text-[15px] font-bold">关闭 MCP Bridge?</div>
            <div className="mt-1.5 text-[12.5px] leading-relaxed text-muted-foreground">
              后台运行会隐藏窗口并保持网关与隧道在线,ChatGPT 可继续使用;
              之后可从托盘图标(右键 Exit)彻底退出。
            </div>
            <div className="mt-4 flex items-center gap-2">
              <Button
                variant="outline"
                className="h-[26px] px-3 text-[12.5px] text-[#FF3B30]"
                onClick={() => bridgeRef.current?.quitApp()}
              >
                直接退出
              </Button>
              <div className="ml-auto flex gap-2">
                <Button variant="outline" className="h-[26px] px-3 text-[12.5px]" onClick={() => setCloseAsked(false)}>
                  取消
                </Button>
                <Button className="h-[26px] px-3 text-[12.5px]" onClick={() => bridgeRef.current?.hideWindow()}>
                  后台运行
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
