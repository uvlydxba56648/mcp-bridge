import { Check, RefreshCw, TriangleAlert } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type { DetectInfo } from "@/lib/bridge"

export type DetectStatus = "checking" | "connected" | "failed"

export function ScreenConnect({
  status,
  info,
  failReason,
  onRescan,
  transport,
  onTransport,
  manual,
  onManual,
  onParse,
  parsing,
  onShowTools,
}: {
  status: DetectStatus
  info: DetectInfo | null
  failReason: string
  onRescan: () => void
  transport: string
  onTransport: (v: string) => void
  manual: string
  onManual: (v: string) => void
  onParse: () => void
  parsing: boolean
  onShowTools: () => void
}) {
  return (
    <div className="px-14 pt-2.5">
      <h1 className="text-[21px] font-bold tracking-tight">连接 JetBrains IDE</h1>
      <p className="mt-1 mb-4 text-muted-foreground">启动时自动检测本机 MCP Server,也可手动粘贴配置。</p>

      <div className="rounded-[10px] border border-[#E2E2E6] bg-card">
        <div className="flex items-center gap-2.5 px-3.5 py-3">
          {status === "connected" && info && (
            <>
              <span className="flex h-5 w-5 flex-none items-center justify-center rounded-full bg-[#28C840] text-white">
                <Check className="h-3 w-3" strokeWidth={3} />
              </span>
              <div>
                <div className="font-semibold">
                  已连接 · {info.name.replace(" MCP Server", "")} {info.version}
                </div>
                <div className="font-mono text-[12px] text-muted-foreground">
                  {info.endpoint.replace(/^https?:\/\//, "")} · {info.tool_count} 个工具 · {info.probe_ms} ms
                </div>
              </div>
              <div className="ml-auto flex gap-1.5">
                <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={onShowTools}>
                  {info.tool_count} 个工具 ▸
                </Button>
                <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={onRescan}>
                  <RefreshCw className="mr-1 h-3 w-3" />
                  重新检测
                </Button>
              </div>
            </>
          )}

          {status === "checking" && (
            <>
              <RefreshCw className="h-4 w-4 animate-spin text-primary" />
              <span className="text-muted-foreground">正在检测本机 MCP Server…</span>
            </>
          )}

          {status === "failed" && (
            <>
              <span className="flex h-5 w-5 flex-none items-center justify-center rounded-full bg-[#FFF3E0] text-[#D87700]">
                <TriangleAlert className="h-3 w-3" strokeWidth={2.5} />
              </span>
              <div className="min-w-0">
                <div className="font-semibold">未检测到 MCP Server</div>
                <div className="truncate text-[12px] text-muted-foreground">
                  请在 IDE 中开启:Settings → Tools → MCP Server(每 3 秒自动重试)
                </div>
              </div>
              <div className="ml-auto flex-none">
                <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={onRescan}>
                  <RefreshCw className="mr-1 h-3 w-3" />
                  重新检测
                </Button>
              </div>
            </>
          )}
        </div>
        {status === "failed" && failReason && (
          <div className="border-t border-[#F0F0F3] px-3.5 py-2 font-mono text-[11px] break-all text-[#D87700]">
            {failReason}
          </div>
        )}
      </div>

      <div className="mx-0.5 mt-3.5 mb-1.5 text-[12px] font-semibold text-muted-foreground">传输协议</div>
      <ToggleGroup
        type="single"
        value={transport}
        onValueChange={(v) => v && onTransport(v)}
        className="justify-start gap-0 rounded-[7px] bg-[#E5E5EA] p-[1.5px]"
      >
        {["stream", "sse"].map((v) => (
          <ToggleGroupItem
            key={v}
            value={v}
            className="h-[24px] rounded-[5.5px] px-3.5 text-[12px] data-[state=on]:bg-white data-[state=on]:shadow-sm"
          >
            {v === "stream" ? "Streamable HTTP" : "SSE"}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>

      <div className="mx-0.5 mt-3.5 mb-1.5 text-[12px] font-semibold text-muted-foreground">手动配置</div>
      <div className="flex gap-2">
        <Input
          value={manual}
          onChange={(e) => onManual(e.target.value)}
          placeholder='{ "type": "sse", "url": "http://127.0.0.1:64342/sse", "headers": {} }'
          className="h-[27px] font-mono text-[12px]"
        />
        <Button variant="outline" className="h-[26px] px-3 text-[12.5px]" disabled={parsing} onClick={onParse}>
          {parsing ? "连接中…" : "解析并连接"}
        </Button>
      </div>
      <div className="mt-1.5 text-[11.5px] text-muted-foreground">
        粘贴 IDE 中 Settings → Tools → MCP Server 的 Copy SSE / HTTP Stream Config
      </div>
    </div>
  )
}
