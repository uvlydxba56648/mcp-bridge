import type { ReactNode } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { cn } from "@/lib/utils"

export type LogLevel = "INFO" | "OK" | "WARN" | "ERR"

export interface LogEntry {
  time: string
  level: LogLevel
  source: string
  message: ReactNode
}

const LEVEL_STYLE: Record<LogLevel, string> = {
  INFO: "bg-[#F0F0F3] text-[#6E6E73]",
  OK: "bg-[#E5F8EB] text-[#1BAC2B]",
  WARN: "bg-[#FFF3E0] text-[#D87700]",
  ERR: "bg-[#FFEBE9] text-[#FF3B30]",
}

/** 格式化日志窗口:时间 | 级别 | 来源 | 消息 四列,非文本框堆砌 */
export function LogView({
  logs,
  onClear,
  onExport,
  className,
}: {
  logs: LogEntry[]
  onClear: () => void
  onExport: () => void
  className?: string
}) {
  return (
    <div className={cn("mt-2.5 flex flex-col overflow-hidden rounded-[10px] border border-[#E2E2E6] bg-card", className)}>
      <div className="flex items-center border-b border-[#F0F0F3] bg-[#FAFAFC] px-3.5 py-[7px]">
        <span className="text-[12px] font-semibold text-[#3A3A3C]">日志</span>
        <div className="ml-auto flex gap-2.5">
          <button className="text-[11.5px] text-primary hover:underline" onClick={onExport}>
            导出
          </button>
          <button className="text-[11.5px] text-primary hover:underline" onClick={onClear}>
            清空
          </button>
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {logs.map((l, i) => (
          <div
            key={i}
            className="flex items-center gap-2.5 border-b border-[#F6F6F8] px-3.5 py-[5px] last:border-0"
          >
            <span className="w-[58px] flex-none font-mono text-[11px] text-muted-foreground">{l.time}</span>
            <span
              className={cn(
                "w-[44px] flex-none rounded px-1.5 py-px text-center text-[10px] font-semibold",
                LEVEL_STYLE[l.level]
              )}
            >
              {l.level}
            </span>
            <span className="w-[34px] flex-none text-[11px] text-muted-foreground">{l.source}</span>
            <span className="truncate text-[12px] text-foreground">{l.message}</span>
          </div>
        ))}
        {logs.length === 0 && (
          <div className="py-8 text-center text-[12px] text-muted-foreground">暂无日志</div>
        )}
      </ScrollArea>
    </div>
  )
}
