import { Badge } from "./badge";
import type { LifecycleTransition } from "@/lib/client";

interface TransitionStatusBadgeProps {
  transition: LifecycleTransition;
  className?: string;
}

type TransitionStatus = "pending" | "in_progress" | "completed" | "failed";

function getTransitionStatus(transition: LifecycleTransition): TransitionStatus {
  // If there's an error message or success is explicitly false, it's failed
  if (transition.error_message || transition.success === false) {
    return "failed";
  }

  // If completed_at is set and success is true (or not set, assuming success), it's completed
  if (transition.completed_at) {
    return "completed";
  }

  // If started_at is set but not completed_at, it's in progress
  if (transition.started_at) {
    return "in_progress";
  }

  // Otherwise it's pending
  return "pending";
}

const statusConfig: Record<TransitionStatus, { variant: "status-new" | "status-provisioning" | "status-provisioned" | "destructive"; label: string }> = {
  pending: { variant: "status-new", label: "Pending" },
  in_progress: { variant: "status-provisioning", label: "In Progress" },
  completed: { variant: "status-provisioned", label: "Completed" },
  failed: { variant: "destructive", label: "Failed" },
};

export function TransitionStatusBadge({ transition, className }: TransitionStatusBadgeProps) {
  const status = getTransitionStatus(transition);
  const config = statusConfig[status];

  return <Badge variant={config.variant} className={className}>{config.label}</Badge>;
}
