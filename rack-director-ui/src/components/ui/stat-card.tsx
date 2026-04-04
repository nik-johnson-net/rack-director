import { cn } from "@/lib/utils";

type StatCardStatus = "new" | "unprovisioned" | "provisioned" | "broken" | "removed" | "transitioning" | "default";

interface StatCardProps {
  label: string;
  value: string | number;
  detail?: string;
  status?: StatCardStatus;
  onClick?: () => void;
  className?: string;
}

const statusLabelColors: Record<StatCardStatus, string> = {
  new: "text-status-new",
  unprovisioned: "text-status-unprovisioned",
  provisioned: "text-status-provisioned",
  broken: "text-status-broken",
  removed: "text-status-removed",
  transitioning: "text-status-transitioning",
  default: "text-text-secondary",
};

export function StatCard({ label, value, detail, status = "default", onClick, className }: StatCardProps) {
  const labelColor = statusLabelColors[status];

  return (
    <div
      onClick={onClick}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={onClick ? (e) => (e.key === "Enter" || e.key === " ") && onClick() : undefined}
      className={cn(
        "bg-bg-surface border border-border p-4",
        "transition-colors",
        onClick && "cursor-pointer hover:bg-bg-raised hover:border-border-muted",
        className
      )}
    >
      <div className={cn("text-xs font-semibold uppercase tracking-wide mb-2", labelColor)}>
        {label}
      </div>
      <div className="text-3xl font-bold text-text-primary leading-none mb-1">
        {value}
      </div>
      {detail && (
        <div className="text-xs text-text-secondary mt-1">{detail}</div>
      )}
    </div>
  );
}
