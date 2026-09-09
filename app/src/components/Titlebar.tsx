/** macOS 风格标题栏:交通灯仅在 macOS / 浏览器预览时显示;Windows 用原生标题栏 */
import { useState } from "react"
import { MoreHorizontal } from "lucide-react"
import { isTauri } from "@/lib/bridge"

const showLights = !isTauri || navigator.userAgent.includes("Mac")

export function Titlebar({ onCleanup }: { onCleanup?: () => void }) {
  const [menuOpen, setMenuOpen] = useState(false)
  return (
    <div
      data-tauri-drag-region
      className="relative flex h-[38px] flex-none items-center border-b border-[#E0E0E3] bg-[#FAFAFC]"
    >
      {showLights && (
        <div className="flex gap-2 pl-3">
          <i className="block h-3 w-3 rounded-full border border-[#E0443E] bg-[#FF5F57]" />
          <i className="block h-3 w-3 rounded-full border border-[#DE9A12] bg-[#FEBC2E]" />
          <i className="block h-3 w-3 rounded-full border border-[#1BAC2B] bg-[#28C840]" />
        </div>
      )}
      <div className="pointer-events-none absolute inset-x-0 text-center text-[13px] font-semibold text-[#3A3A3C]">
        MCP Bridge
      </div>
      {onCleanup && (
        <div className="relative ml-auto pr-2">
          <button
            className="flex h-6 w-6 items-center justify-center rounded-md text-[#6E6E73] hover:bg-[#ECECEF]"
            onClick={() => setMenuOpen((v) => !v)}
            onBlur={() => setTimeout(() => setMenuOpen(false), 150)}
            title="更多"
          >
            <MoreHorizontal className="h-4 w-4" />
          </button>
          {menuOpen && (
            <div className="absolute top-7 right-0 z-50 w-44 rounded-lg border border-[#E2E2E6] bg-white py-1 shadow-lg">
              <button
                className="w-full px-3 py-1.5 text-left text-[12.5px] text-[#FF3B30] hover:bg-[#F5F5F7]"
                onClick={() => {
                  setMenuOpen(false)
                  onCleanup()
                }}
              >
                卸载已安装的组件
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
