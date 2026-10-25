import * as React from "react"

import { cn } from "@/lib/cn"

function Input({ className, type = "text", ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "h-11 w-full rounded-field border border-ink-200 bg-white px-3.5 text-sm text-ink-900 shadow-xs transition-colors",
        "placeholder:text-ink-400",
        "hover:border-ink-300",
        "focus-visible:border-brand-500 focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-brand-500",
        "disabled:cursor-not-allowed disabled:bg-ink-50 disabled:text-ink-400",
        "aria-invalid:border-danger-500 aria-invalid:focus-visible:outline-danger-500",
        className,
      )}
      {...props}
    />
  )
}

export { Input }
