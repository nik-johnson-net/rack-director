import { Badge } from "./badge";
import type { DeviceLifecycle } from "@/lib/client";

interface StatusBadgeProps {
  status: DeviceLifecycle;
  className?: string;
}

const statusConfig = {
  new: { variant: "status-new" as const, label: "New" },
  unprovisioned: { variant: "status-unprovisioned" as const, label: "Unprovisioned" },
  provisioned: { variant: "status-provisioned" as const, label: "Provisioned" },
  broken: { variant: "status-broken" as const, label: "Broken" },
  removed: { variant: "status-removed" as const, label: "Removed" },
};

export function StatusBadge({ status, className }: StatusBadgeProps) {
  const config = statusConfig[status] || statusConfig.new;
  return <Badge variant={config.variant} className={className}>{config.label}</Badge>;
}
