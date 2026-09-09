import { Loader2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"

export interface ProviderSpec {
  id: string
  name: string
  tag?: string
  desc: string
  bin: string
  installCmd: string | null // null = 无包管理,软件自动下载
  credHint: string | null // 凭据提示;null = 不需要
  needTunnelId?: boolean
}

export const PROVIDERS: ProviderSpec[] = [
  {
    id: "cloudflare",
    name: "Cloudflare Tunnel",
    tag: "默认",
    desc: "免费 · 自带 HTTPS · 无需账号",
    bin: "cloudflared",
    installCmd: "brew install cloudflared",
    credHint: null,
  },
  {
    id: "ngrok",
    name: "ngrok",
    desc: "需要 authtoken,免费版域名会变",
    bin: "ngrok",
    installCmd: "brew install ngrok",
    credHint: "authtoken(必填)",
  },
  {
    id: "openai",
    name: "OpenAI Tunnel",
    desc: "官方 tunnel-client,零公网入口",
    bin: "tunnel-client",
    installCmd: null,
    credHint: "Platform API Key(必填)",
    needTunnelId: true,
  },
]

export function ScreenTunnel({
  provider,
  onProvider,
  installed,
  installing,
  onConnect,
  onCopyCmd,
  token,
  onToken,
  tunnelId,
  onTunnelId,
  tunnelUp,
}: {
  provider: string
  onProvider: (v: string) => void
  installed: boolean
  installing: boolean
  onConnect: () => void
  onCopyCmd: (cmd: string) => void
  token: string
  onToken: (v: string) => void
  tunnelId: string
  onTunnelId: (v: string) => void
  tunnelUp: boolean
}) {
  const spec = PROVIDERS.find((p) => p.id === provider) ?? PROVIDERS[0]
  const credReady = spec.credHint === null || (token.length > 0 && (!spec.needTunnelId || tunnelId.length > 0))

  return (
    <div className="px-14 pt-2.5">
      <h1 className="text-[21px] font-bold tracking-tight">隧道穿透</h1>
      <p className="mt-1 mb-4 text-muted-foreground">选择将本机 MCP 暴露到公网的方式,缺少的组件会自动安装。</p>

      <RadioGroup value={provider} onValueChange={onProvider} className="gap-0 overflow-hidden rounded-[10px] border border-[#E2E2E6] bg-card">
        {PROVIDERS.map((p) => (
          <label
            key={p.id}
            className="flex cursor-pointer items-center gap-2.5 border-b border-[#F0F0F3] px-3.5 py-2.5 last:border-0"
          >
            <RadioGroupItem value={p.id} className="border-[#B9B9BE] text-primary" />
            <span className="font-medium">{p.name}</span>
            {p.tag && (
              <span className="rounded bg-[#F0F0F3] px-1.5 py-px text-[10.5px] text-muted-foreground">{p.tag}</span>
            )}
            <span className="ml-auto text-right text-[12px] text-muted-foreground">{p.desc}</span>
          </label>
        ))}
      </RadioGroup>

      <div className="mx-0.5 mt-3.5 mb-1.5 text-[12px] font-semibold text-muted-foreground">
        {spec.name.toUpperCase()}
      </div>
      <div className="overflow-hidden rounded-[10px] border border-[#E2E2E6] bg-card">
        {/* 组件状态:系统有就用系统的 */}
        <div className="flex items-center gap-2.5 border-b border-[#F0F0F3] px-3.5 py-2.5">
          <div className="w-[76px] flex-none text-right text-muted-foreground">组件</div>
          <code className="font-mono text-[12px]">{spec.bin}</code>
          {installed ? (
            <span className="text-[12px] text-[#1BAC2B]">✓ 已就绪</span>
          ) : installing ? (
            <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin text-primary" /> 正在自动安装…
            </span>
          ) : (
            <span className="text-[12px] text-[#D87700]">● 未安装(将自动下载,或先用系统 PATH 里的)</span>
          )}
        </div>
        {/* 安装命令(未安装时展示) */}
        {!installed && spec.installCmd && (
          <div className="flex items-center gap-2.5 border-b border-[#F0F0F3] px-3.5 py-2.5">
            <div className="w-[76px] flex-none text-right text-muted-foreground">安装命令</div>
            <Input readOnly value={spec.installCmd} className="h-[27px] flex-1 font-mono text-[12px]" />
            <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={() => onCopyCmd(spec.installCmd!)}>
              复制
            </Button>
          </div>
        )}
        {/* 凭据 */}
        {spec.credHint && (
          <div className="flex items-center gap-2.5 border-b border-[#F0F0F3] px-3.5 py-2.5">
            <div className="w-[76px] flex-none text-right text-muted-foreground">凭据</div>
            <Input
              type="password"
              value={token}
              onChange={(e) => onToken(e.target.value)}
              placeholder={spec.credHint}
              className="h-[27px] flex-1 font-mono text-[12px]"
            />
          </div>
        )}
        {spec.needTunnelId && (
          <div className="flex items-center gap-2.5 border-b border-[#F0F0F3] px-3.5 py-2.5">
            <div className="w-[76px] flex-none text-right text-muted-foreground">Tunnel ID</div>
            <Input
              value={tunnelId}
              onChange={(e) => onTunnelId(e.target.value)}
              placeholder="Platform → Settings → Tunnels 中创建"
              className="h-[27px] flex-1 font-mono text-[12px]"
            />
          </div>
        )}
        {/* 隧道状态 */}
        <div className="flex items-center gap-2.5 px-3.5 py-2.5">
          <div className="w-[76px] flex-none text-right text-muted-foreground">隧道状态</div>
          {tunnelUp ? (
            <span className="flex items-center gap-1.5 text-[12px] text-[#1BAC2B]">
              <i className="h-2 w-2 rounded-full bg-[#28C840]" />
              {provider === "openai" ? "已连接(零公网入口)" : "已连接"}
            </span>
          ) : installing ? (
            <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin text-primary" /> 正在安装并建立连接…
            </span>
          ) : (
            <>
              <span className="text-[12px] text-muted-foreground">待连接</span>
              <Button
                size="sm"
                className="ml-auto h-[23px] px-2.5 text-[12px]"
                disabled={!credReady}
                onClick={onConnect}
              >
                一键安装并连接
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
