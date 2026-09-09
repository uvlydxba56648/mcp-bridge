import { cn } from "@/lib/utils"

const STEPS = ["连接 IDE", "隧道穿透", "生成密钥"]

export function StepIndicator({ step }: { step: number }) {
  return (
    <div className="flex items-center justify-center gap-1.5 pt-3 text-[11px] text-[#8E8E93]">
      {STEPS.map((label, i) => (
        <div key={label} className="flex items-center gap-1.5">
          {i > 0 && <span className="mx-1">—</span>}
          <span className={cn(i === step && "font-medium text-[#1D1D1F]")}>{label}</span>
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              i === step ? "bg-primary" : i < step ? "bg-[#8E8E93]" : "bg-[#D1D1D6]"
            )}
          />
        </div>
      ))}
    </div>
  )
}
