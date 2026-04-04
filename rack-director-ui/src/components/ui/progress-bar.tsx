import { cn } from "@/lib/utils";

interface ProgressBarProps {
  value: number;
  className?: string;
}

export function ProgressBar({ value, className }: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, value));

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <div className="w-20 h-1.5 bg-bg-overlay rounded-sm overflow-hidden">
        <div
          className="h-full bg-accent rounded-sm transition-all"
          style={{ width: `${clamped}%` }}
        />
      </div>
      <span className="text-xs text-text-secondary">{clamped}%</span>
    </div>
  );
}
