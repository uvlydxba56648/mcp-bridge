import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { LogView, type LogEntry } from "@/components/LogView"
import { TOOL_COUNT, ENABLED_COUNT } from "@/lib/toolmeta"

export function ScreenReady({
  url,
  provider,
  tunnelId,
  onCopy,
  logs,
  onClearLogs,
  onExportLogs,
}: {
  url: string
  provider: string
  tunnelId: string
  onCopy: (what: string, text: string) => void
  logs: LogEntry[]
  onClearLogs: () => void
  onExportLogs: () => void
}) {
  return (
    <div className="flex h-full flex-col px-14 pt-1.5 pb-2">
      <div className="mt-1.5 flex items-center gap-2.5">
        <span className="h-[11px] w-[11px] flex-none rounded-full bg-[#28C840] ring-[3px] ring-[#28C840]/20" />
        <h1 className="text-[21px] font-bold tracking-tight">已就绪,可配置到 ChatGPT</h1>
      </div>
      <p className="mt-1 mb-3 ml-[21px] text-[12.5px] text-muted-foreground">
        端到端自检通过 · 工具 {ENABLED_COUNT} / {TOOL_COUNT} 全部放开
      </p>

      <div className="overflow-hidden rounded-[10px] border border-[#E2E2E6] bg-card">
        {provider === "openai" ? (
          <div className="flex items-center gap-2.5 px-3.5 py-2.5">
            <div className="w-[76px] flex-none text-right text-muted-foreground">Tunnel ID</div>
            <Input readOnly value={tunnelId} className="h-[27px] flex-1 font-mono text-[12px]" />
            <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={() => onCopy("Tunnel ID", tunnelId)}>
              复制
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-2.5 px-3.5 py-2.5">
            <div className="w-[76px] flex-none text-right text-muted-foreground">公网地址</div>
            <Input readOnly value={url} className="h-[27px] flex-1 font-mono text-[12px]" />
            <Button variant="outline" size="sm" className="h-[23px] px-2.5 text-[12px]" onClick={() => onCopy("公网地址", url)}>
              复制
            </Button>
          </div>
        )}
      </div>

      <div className="mt-2 text-center text-[11.5px] text-[#8E8E93]">
        {provider === "openai" ? (
          <>粘贴到 <b className="font-medium text-[#6E6E73]">ChatGPT → Apps → 创建 → Connection 选 Tunnel</b></>
        ) : (
          <>粘贴到 <b className="font-medium text-[#6E6E73]">ChatGPT → 设置 → Apps → 创建</b>,身份验证选 OAuth,浏览器会弹授权页</>
        )}
      </div>

      <LogView logs={logs} onClear={onClearLogs} onExport={onExportLogs} className="min-h-[120px] flex-1" />
    </div>
  )
}
