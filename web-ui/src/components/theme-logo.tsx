import { useId } from "react"

import { cn } from "@/lib/cn"

export interface ThemeLogoProps {
  className?: string
}

/** 产品标记：立方体 + 流转箭头，暗底/亮底通用 */
export function ThemeLogo({ className }: ThemeLogoProps) {
  // 同一页面可能存在多个实例，渐变 id 必须唯一，否则引用会串到隐藏实例上
  const gradientId = `rts-logo-grad-${useId().replace(/:/g, "")}`

  return (
    <svg
      viewBox="0 0 40 40"
      role="img"
      aria-label="RTS 搬运工"
      className={cn("size-9", className)}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#38bdf8" />
          <stop offset="100%" stopColor="#0369a1" />
        </linearGradient>
      </defs>
      <rect x="2" y="2" width="36" height="36" rx="10" fill={`url(#${gradientId})`} />
      <path
        d="M11 14.5h10.5a4 4 0 0 1 0 8H14"
        fill="none"
        stroke="#fff"
        strokeWidth="2.6"
        strokeLinecap="round"
      />
      <path
        d="M17.5 19 13.5 22.5l4 3.5"
        fill="none"
        stroke="#fff"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="28.5" cy="18.5" r="2.4" fill="#fff" />
    </svg>
  )
}
