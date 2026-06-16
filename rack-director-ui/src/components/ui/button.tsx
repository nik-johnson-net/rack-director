import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap text-xs font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-3.5 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:ring-1 focus-visible:ring-accent cursor-pointer",
  {
    variants: {
      variant: {
        // Primary: accent bg, dark text
        default:
          "bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover hover:border-accent-hover",
        // Secondary/Default: surface bg, primary text, border
        secondary:
          "bg-bg-surface text-text-primary border border-border rounded hover:bg-bg-raised",
        // Danger: surface bg, red text, error border
        danger:
          "bg-bg-surface text-status-broken border border-error-border rounded hover:bg-error-bg",
        // Ghost: transparent, secondary text
        ghost:
          "bg-transparent text-text-secondary border border-transparent rounded hover:bg-bg-raised hover:text-text-primary",
        // Outline (kept for compatibility)
        outline:
          "bg-bg-surface text-text-primary border border-border rounded hover:bg-bg-raised",
        // Destructive (alias for danger, kept for compatibility)
        destructive:
          "bg-bg-surface text-status-broken border border-error-border rounded hover:bg-error-bg",
        // Link variant
        link: "text-accent underline-offset-4 hover:text-accent-hover hover:underline border-transparent bg-transparent",
      },
      size: {
        default: "h-8 px-4 py-2",
        sm: "h-7 px-3 py-1",
        lg: "h-9 px-6 py-2",
        icon: "size-8",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot : "button"

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export { Button, buttonVariants }
