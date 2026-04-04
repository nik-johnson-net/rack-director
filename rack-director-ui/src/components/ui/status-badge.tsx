import { cn } from "@/lib/utils";
import type { DeviceLifecycle } from "@/lib/client";

interface StatusBadgeProps {
  status: DeviceLifecycle | "transitioning";
  className?: string;
}

const statusConfig: Record<string, { dotClass: string; bgClass: string; textClass: string; label: string }> = {
  new: {
    dotClass: "bg-status-new",
    bgClass: "bg-status-new-bg",
    textClass: "text-status-new",
    label: "New",
  },
  unprovisioned: {
    dotClass: "bg-status-unprovisioned",
    bgClass: "bg-status-unprovisioned-bg",
    textClass: "text-status-unprovisioned",
    label: "Unprovisioned",
  },
  provisioned: {
    dotClass: "bg-status-provisioned",
    bgClass: "bg-status-provisioned-bg",
    textClass: "text-status-provisioned",
    label: "Provisioned",
  },
  broken: {
    dotClass: "bg-status-broken",
    bgClass: "bg-status-broken-bg",
    textClass: "text-status-broken",
    label: "Broken",
  },
  removed: {
    dotClass: "bg-status-removed",
    bgClass: "bg-status-removed-bg",
    textClass: "text-status-removed",
    label: "Removed",
  },
  transitioning: {
    dotClass: "bg-status-transitioning",
    bgClass: "bg-status-transitioning-bg",
    textClass: "text-status-transitioning",
    label: "Transitioning",
  },
};

export function StatusBadge({ status, className }: StatusBadgeProps) {
  const config = statusConfig[status] ?? statusConfig.new;
  const isTransitioning = status === "transitioning";

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-sm text-xs font-medium",
        config.bgClass,
        config.textClass,
        className
      )}
    >
      <span
        className={cn(
          "size-1.5 rounded-full shrink-0",
          config.dotClass,
          isTransitioning && "animate-pulse-dot"
        )}
      />
      {config.label}
    </span>
  );
}
