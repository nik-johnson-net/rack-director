import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex items-center justify-center rounded-sm px-2 py-0.5 text-xs font-medium w-fit whitespace-nowrap shrink-0 [&>svg]:size-3 gap-1 [&>svg]:pointer-events-none overflow-hidden",
  {
    variants: {
      variant: {
        default:
          "bg-accent-muted text-accent border border-accent/20",
        secondary:
          "bg-bg-raised text-text-secondary border border-border",
        destructive:
          "bg-error-bg text-status-broken border border-error-border/30",
        outline:
          "bg-transparent text-text-secondary border border-border",
        // Status variants using CSS custom properties directly
        "status-new":
          "bg-status-new-bg text-status-new border-0",
        "status-unprovisioned":
          "bg-status-unprovisioned-bg text-status-unprovisioned border-0",
        "status-provisioned":
          "bg-status-provisioned-bg text-status-provisioned border-0",
        "status-broken":
          "bg-status-broken-bg text-status-broken border-0",
        "status-removed":
          "bg-status-removed-bg text-status-removed border-0",
        "status-provisioning":
          "bg-status-transitioning-bg text-status-transitioning border-0 animate-pulse",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Badge({
  className,
  variant,
  asChild = false,
  ...props
}: React.ComponentProps<"span"> &
  VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "span"

  return (
    <Comp
      data-slot="badge"
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export { Badge, badgeVariants }
