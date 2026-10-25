import { useCallback, type ComponentProps, type KeyboardEvent } from "react"
import { Loader2 } from "lucide-react"

import { Input } from "@/components/ui/input"
import { cn } from "@/lib/cn"

export interface CaptchaFieldProps extends ComponentProps<"input"> {
  /** 验证码图片 data URI，null 表示尚未取到 */
  image: string | null
  /** 验证码请求中 */
  loading: boolean
  /** 点击图片或按 Enter/Space 刷新验证码 */
  onRefresh: () => void
}

/**
 * 验证码字段：左侧输入框 + 右侧可点击刷新的验证码图片。
 * 图片为可聚焦按钮语义（role="button" + tabIndex=0），支持键盘 Enter/Space 刷新。
 */
export function CaptchaField({
  className,
  image,
  loading,
  onRefresh,
  disabled,
  ...props
}: CaptchaFieldProps) {
  const handleClick = useCallback(() => {
    if (!disabled && !loading) onRefresh()
  }, [disabled, loading, onRefresh])

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLImageElement>) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault()
        if (!disabled && !loading) onRefresh()
      }
    },
    [disabled, loading, onRefresh],
  )

  return (
    <div className="flex items-start gap-3">
      <Input
        id="captchaCode"
        autoComplete="off"
        inputMode="text"
        maxLength={8}
        placeholder="请输入图中字符"
        className={cn("min-w-0 flex-1", className)}
        disabled={disabled || loading}
        {...props}
      />
      <div className="h-15 w-40 shrink-0 overflow-hidden rounded-field border border-ink-200 bg-white">
        {loading || !image ? (
          <span className="flex h-full w-full items-center justify-center bg-ink-50 text-ink-400">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          </span>
        ) : (
          <img
            src={image}
            alt="验证码"
            role="button"
            tabIndex={0}
            aria-label="点击刷新验证码"
            aria-disabled={disabled || undefined}
            title="点击刷新验证码"
            onClick={handleClick}
            onKeyDown={handleKeyDown}
            className="h-full w-full cursor-pointer object-contain select-none disabled:cursor-not-allowed"
            draggable={false}
          />
        )}
      </div>
    </div>
  )
}
