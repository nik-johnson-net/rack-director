import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex items-center justify-center rounded-full border px-2 py-0.5 text-xs font-medium w-fit whitespace-nowrap shrink-0 [&>svg]:size-3 gap-1 [&>svg]:pointer-events-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive transition-[color,box-shadow] overflow-hidden",
  {
    variants: {
      variant: {
        default:
          "border-transparent bg-primary text-primary-foreground [a&]:hover:bg-primary/90",
        secondary:
          "border-transparent bg-secondary text-secondary-foreground [a&]:hover:bg-secondary/90",
        destructive:
          "border-transparent bg-destructive text-white [a&]:hover:bg-destructive/90 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 dark:bg-destructive/60",
        outline:
          "text-foreground [a&]:hover:bg-accent [a&]:hover:text-accent-foreground",
        "status-new":
          "bg-[oklch(var(--status-new-bg))] text-[oklch(var(--status-new))] border-[oklch(var(--status-new-border))]",
        "status-unprovisioned":
          "bg-[oklch(var(--status-unprovisioned-bg))] text-[oklch(var(--status-unprovisioned))] border-[oklch(var(--status-unprovisioned-border))]",
        "status-provisioned":
          "bg-[oklch(var(--status-provisioned-bg))] text-[oklch(var(--status-provisioned))] border-[oklch(var(--status-provisioned-border))]",
        "status-broken":
          "bg-[oklch(var(--status-broken-bg))] text-[oklch(var(--status-broken))] border-[oklch(var(--status-broken-border))]",
        "status-removed":
          "bg-[oklch(var(--status-removed-bg))] text-[oklch(var(--status-removed))] border-[oklch(var(--status-removed-border))]",
        "status-provisioning":
          "bg-[oklch(var(--status-provisioning-bg))] text-[oklch(var(--status-provisioning))] border-[oklch(var(--status-provisioning-border))] animate-pulse",
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

export { Badge, badgeVariants }
