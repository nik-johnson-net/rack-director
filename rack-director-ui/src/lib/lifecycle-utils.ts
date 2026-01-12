import type { DeviceLifecycle } from "./client";

export function getLifecycleVariant(lifecycle: DeviceLifecycle) {
  const variants = {
    new: "status-new",
    unprovisioned: "status-unprovisioned",
    provisioned: "status-provisioned",
    broken: "status-broken",
    removed: "status-removed",
  } as const;

  return variants[lifecycle] || "status-new";
}

export function getLifecycleLabel(lifecycle: DeviceLifecycle): string {
  const labels = {
    new: "New",
    unprovisioned: "Unprovisioned",
    provisioned: "Provisioned",
    broken: "Broken",
    removed: "Removed",
  };

  return labels[lifecycle] || lifecycle;
}

export function isActiveLifecycle(lifecycle: DeviceLifecycle): boolean {
  return lifecycle !== "removed";
}
