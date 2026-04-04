import { cn } from "@/lib/utils";
import type { ElementType } from "react";

interface EmptyStateAction {
  label: string;
  onClick: () => void;
}

interface EmptyStateProps {
  icon?: ElementType;
  title: string;
  description?: string;
  action?: EmptyStateAction;
  className?: string;
}

export function EmptyState({ icon: Icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div className={cn("flex flex-col items-center justify-center py-8 text-center", className)}>
      {Icon && (
        <Icon className="size-8 text-text-muted mb-3 opacity-50" />
      )}
      <p className="text-lg font-medium text-text-primary mb-1">{title}</p>
      {description && (
        <p className="text-sm text-text-secondary max-w-sm">{description}</p>
      )}
      {action && (
        <button
          onClick={action.onClick}
          className="mt-4 inline-flex items-center gap-2 px-4 py-2 text-xs font-medium bg-accent text-bg-base rounded hover:bg-accent-hover transition-colors cursor-pointer"
        >
          {action.label}
        </button>
      )}
    </div>
  );
}
