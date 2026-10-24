import { useState, type ComponentProps } from "react"
import { Eye, EyeOff } from "lucide-react"

import { Input } from "@/components/ui/input"
import { cn } from "@/lib/cn"

export interface PasswordFieldProps extends ComponentProps<"input"> {
  invalid?: boolean
}

/** 密码输入框，带显示/隐藏切换 */
export function PasswordField({ className, invalid, ...props }: PasswordFieldProps) {
  const [visible, setVisible] = useState(false)

  return (
    <div className="relative">
      <Input
        type={visible ? "text" : "password"}
        className={cn("pr-11", className)}
        aria-invalid={invalid || undefined}
        {...props}
      />
      <button
        type="button"
        onClick={() => setVisible((value) => !value)}
        aria-label={visible ? "隐藏密码" : "显示密码"}
        aria-pressed={visible}
        className="absolute inset-y-0 right-0 flex w-11 items-center justify-center rounded-r-field text-ink-400 transition-colors hover:text-ink-700 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-brand-500"
      >
        {visible ? (
          <EyeOff className="size-4" aria-hidden="true" />
        ) : (
          <Eye className="size-4" aria-hidden="true" />
        )}
      </button>
    </div>
  )
}
