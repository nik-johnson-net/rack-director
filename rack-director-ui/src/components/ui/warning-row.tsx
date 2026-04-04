import { X } from "lucide-react";
import { cn } from "@/lib/utils";

interface WarningRowProps {
  severity: "error" | "warning";
  device?: string;
  message: string;
  onDismiss?: () => void;
  className?: string;
}

export function WarningRow({ severity, device, message, onDismiss, className }: WarningRowProps) {
  const isError = severity === "error";

  return (
    <div
      className={cn(
        "flex items-center gap-3 px-3 py-2 border-l-[3px]",
        isError ? "bg-error-bg border-status-broken" : "bg-warn-bg border-status-unprovisioned",
        className
      )}
    >
      {device && (
        <span className="text-sm font-medium text-text-primary min-w-[90px] shrink-0">
          {device}
        </span>
      )}
      <span className="text-sm text-text-secondary flex-1">{message}</span>
      {onDismiss && (
        <button
          onClick={onDismiss}
          aria-label="Dismiss"
          className="text-text-muted hover:text-text-secondary transition-colors shrink-0 cursor-pointer"
        >
          <X className="size-3.5" />
        </button>
      )}
    </div>
  );
}
