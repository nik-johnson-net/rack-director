import * as React from "react"

import { cn } from "@/lib/utils"

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "flex h-8 w-full min-w-0 bg-bg-base border border-border rounded-sm px-3 py-1 text-sm text-text-primary placeholder:text-text-muted",
        "transition-colors outline-none",
        "focus:border-accent focus:ring-1 focus:ring-accent/50",
        "aria-invalid:border-status-broken aria-invalid:ring-1 aria-invalid:ring-status-broken/30",
        "disabled:pointer-events-none disabled:opacity-50",
        "file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-text-primary",
        className
      )}
      {...props}
    />
  )
}

export { Input }
