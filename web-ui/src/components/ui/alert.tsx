import * as React from "react"

import { cn } from "@/lib/cn"

const alertVariants = {
  danger:
    "border-danger-200 bg-danger-50 text-danger-700 [&_svg]:text-danger-500",
  info: "border-brand-200 bg-brand-50 text-brand-800 [&_svg]:text-brand-500",
  success:
    "border-emerald-200 bg-emerald-50 text-emerald-700 [&_svg]:text-emerald-500",
} as const

export interface AlertProps extends React.ComponentProps<"div"> {
  variant?: keyof typeof alertVariants
}

function Alert({ className, variant = "danger", ...props }: AlertProps) {
  return (
    <div
      role="alert"
      data-slot="alert"
      className={cn(
        "flex items-start gap-2.5 rounded-field border px-3.5 py-3 text-sm",
        alertVariants[variant],
        className,
      )}
      {...props}
    />
  )
}

export { Alert }
