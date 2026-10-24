import * as React from "react"
import * as CheckboxPrimitive from "@radix-ui/react-checkbox"
import { Check } from "lucide-react"

import { cn } from "@/lib/cn"

function Checkbox({
  className,
  ...props
}: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        "peer size-[18px] shrink-0 rounded-[5px] border border-ink-300 bg-white transition-colors",
        "hover:border-brand-400",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-500",
        "data-[state=checked]:border-brand-500 data-[state=checked]:bg-brand-500 data-[state=checked]:text-white",
        "disabled:cursor-not-allowed disabled:opacity-55",
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator className="flex items-center justify-center text-current">
        <Check className="size-3.5" strokeWidth={3} aria-hidden="true" />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  )
}

export { Checkbox }
