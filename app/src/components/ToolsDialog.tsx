import { useMemo, useState } from "react"
import { Search } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { SERVER, TOOLS, type Risk } from "@/lib/toolmeta"

const RISK_META: Record<Risk, { label: string; dot: string; badge: string }> = {
  read: { label: "只读", dot: "bg-[#1BAC2B]", badge: "bg-[#E5F8EB] text-[#1BAC2B] hover:bg-[#E5F8EB]" },
  write: { label: "写入", dot: "bg-[#D87700]", badge: "bg-[#FFF3E0] text-[#D87700] hover:bg-[#FFF3E0]" },
  exec: { label: "执行", dot: "bg-[#FF3B30]", badge: "bg-[#FFEBE9] text-[#FF3B30] hover:bg-[#FFEBE9]" },
}

export function ToolsDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
}) {
  const [q, setQ] = useState("")
  const list = useMemo(
    () => TOOLS.filter((t) => t.name.toLowerCase().includes(q.trim().toLowerCase())),
    [q]
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent showCloseButton={false} className="gap-0 overflow-hidden p-0 sm:max-w-[540px]">
        <DialogHeader className="flex-row items-center gap-2.5 space-y-0 px-4 pt-3.5 pb-2">
          <DialogTitle className="text-[14px]">工具列表</DialogTitle>
          <DialogDescription className="text-[12px]">
            {TOOLS.length} 个 · 来自 {SERVER.name} {SERVER.version} 实测
          </DialogDescription>
          <div className="relative ml-auto w-[170px]">
            <Search className="absolute top-1/2 left-2 h-3 w-3 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="搜索工具"
              className="h-6 pl-6 text-[12px]"
            />
          </div>
        </DialogHeader>

        <div className="flex gap-3 px-4 pb-2 text-[11px] text-muted-foreground">
          {(["read", "write", "exec"] as Risk[]).map((r) => (
            <span key={r} className="flex items-center gap-1">
              <i className={`h-[7px] w-[7px] rounded-full ${RISK_META[r].dot}`} />
              {RISK_META[r].label}
              {r === "exec" && "(默认关闭)"}
            </span>
          ))}
        </div>

        <Separator />
        <div className="h-[280px] overflow-y-auto">
          {list.map((t) => (
            <div
              key={t.name}
              className="flex items-center gap-2.5 border-b border-[#F6F6F8] px-4 py-[7px] last:border-0"
            >
              <code className="font-mono text-[12px] text-foreground">{t.name}</code>
              <span className="flex-1 truncate text-[11.5px] text-muted-foreground">{t.desc}</span>
              <Badge variant="secondary" className={`rounded-full px-2 py-0 text-[10.5px] ${RISK_META[t.risk].badge}`}>
                {RISK_META[t.risk].label}
              </Badge>
            </div>
          ))}
          {list.length === 0 && (
            <div className="py-10 text-center text-[12px] text-muted-foreground">无匹配工具</div>
          )}
        </div>
        <Separator />

        <div className="flex items-center px-4 py-2.5">
          <span className="text-[11.5px] text-muted-foreground">
            全部 {TOOLS.length} 个工具已放开,执行类(红)请注意风险
          </span>
          <Button size="sm" className="ml-auto h-[23px] px-2.5 text-[12px]" variant="outline" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
