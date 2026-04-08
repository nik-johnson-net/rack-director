import { useEffect, useState } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { PageHeader } from "@/components/ui/page-header";
import { StatCard } from "@/components/ui/stat-card";
import { SectionHeader } from "@/components/ui/section-header";
import { ProgressBar } from "@/components/ui/progress-bar";
import { WarningRow } from "@/components/ui/warning-row";
import { StatusBadge } from "@/components/ui/status-badge";
import { Button } from "@/components/ui/button";
import {
  type Device,
  type DeviceWarning,
  type LifecycleTransition,
  type PendingDevice,
  getDeviceStatus,
  getDeviceWarnings,
  dismissDeviceWarning,
} from "@/lib/client";
import { Activity, Server } from "lucide-react";

interface LoaderData {
  devices: Device[];
  pendingDevices: PendingDevice[];
}

interface ActiveTransitionRow {
  device: Device;
  transition: LifecycleTransition;
}

interface DeviceWarningRow {
  device: Device;
  warning: DeviceWarning;
}

function Index() {
  const { devices, pendingDevices } = useLoaderData() as LoaderData;
  const navigate = useNavigate();

  const [activeTransitions, setActiveTransitions] = useState<ActiveTransitionRow[]>([]);
  const [warnings, setWarnings] = useState<DeviceWarningRow[]>([]);
  const [transitionsLoading, setTransitionsLoading] = useState(true);
  const [warningsLoading, setWarningsLoading] = useState(true);

  // Pending devices waiting for first boot (no completed_at)
  const activePendingDevices = pendingDevices.filter((p) => !p.completed_at);

  // Compute lifecycle counts
  const counts = {
    new: devices.filter((d) => d.lifecycle === "new").length,
    unprovisioned: devices.filter((d) => d.lifecycle === "unprovisioned").length,
    provisioned: devices.filter((d) => d.lifecycle === "provisioned").length,
    broken: devices.filter((d) => d.lifecycle === "broken").length,
    pending: activePendingDevices.length,
  };

  // Fetch active transitions for all devices
  useEffect(() => {
    if (devices.length === 0) {
      setTransitionsLoading(false);
      return;
    }

    let cancelled = false;

    Promise.allSettled(
      devices.map((device) =>
        getDeviceStatus(device.uuid).then((status) => ({
          device,
          status,
        }))
      )
    ).then((results) => {
      if (cancelled) return;
      const rows: ActiveTransitionRow[] = [];
      for (const result of results) {
        if (result.status === "fulfilled" && result.value.status.active_transition) {
          rows.push({
            device: result.value.device,
            transition: result.value.status.active_transition,
          });
        }
      }
      setActiveTransitions(rows);
      setTransitionsLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [devices]);

  // Fetch warnings for all devices
  useEffect(() => {
    if (devices.length === 0) {
      setWarningsLoading(false);
      return;
    }

    let cancelled = false;

    Promise.allSettled(
      devices.map((device) =>
        getDeviceWarnings(device.uuid).then((deviceWarnings) =>
          deviceWarnings.map((warning) => ({ device, warning }))
        )
      )
    ).then((results) => {
      if (cancelled) return;
      const rows: DeviceWarningRow[] = [];
      for (const result of results) {
        if (result.status === "fulfilled") {
          rows.push(...result.value);
        }
      }
      setWarnings(rows);
      setWarningsLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [devices]);

  function handleDismissWarning(device: Device, warningId: number) {
    dismissDeviceWarning(device.uuid, warningId).then(() => {
      setWarnings((prev) =>
        prev.filter((w) => !(w.device.uuid === device.uuid && w.warning.id === warningId))
      );
    }).catch(() => {
      // Silently ignore dismiss errors on dashboard
    });
  }

  function getTransitionAction(transition: LifecycleTransition): string {
    return `${transition.from_state} → ${transition.to_state}`;
  }

  function getTransitionProgress(transition: LifecycleTransition): number {
    // Active transitions in progress — show indeterminate as 50% if no progress info
    if (!transition.started_at) return 0;
    return 50;
  }

  function getDeviceHostname(device: Device): string {
    return device.attributes?.hostname ?? device.uuid.slice(0, 8);
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Dashboard"
        description="Fleet overview and quick actions"
      />

      {/* Stat Cards */}
      <div className="grid gap-px" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))" }}>
        <StatCard
          label="Pending"
          value={counts.pending}
          detail="awaiting first boot"
          status="new"
          onClick={() => navigate("/devices")}
        />
        <StatCard
          label="New"
          value={counts.new}
          detail="awaiting discovery"
          status="new"
          onClick={() => navigate("/devices")}
        />
        <StatCard
          label="Unprovisioned"
          value={counts.unprovisioned}
          detail="ready to provision"
          status="unprovisioned"
          onClick={() => navigate("/devices")}
        />
        <StatCard
          label="Provisioned"
          value={counts.provisioned}
          detail="in service"
          status="provisioned"
          onClick={() => navigate("/devices")}
        />
        <StatCard
          label="Broken"
          value={counts.broken}
          detail="requires attention"
          status="broken"
          onClick={() => navigate("/devices")}
        />
      </div>

      {/* Quick Actions */}
      <div>
        <SectionHeader title="Quick Actions" />
        <div className="flex flex-wrap gap-2">
          <Button variant="default" onClick={() => navigate("/devices")}>
            Provision Device
          </Button>
          <Button variant="secondary" onClick={() => navigate("/devices")}>
            Deprovision Device
          </Button>
          <Button variant="secondary" onClick={() => navigate("/roles/new")}>
            Create Role
          </Button>
          <Button variant="secondary" onClick={() => navigate("/operating-systems/new")}>
            Add OS Image
          </Button>
        </div>
      </div>

      {/* Active Transitions */}
      <div>
        <SectionHeader title="Active Transitions" linkText="View all" linkHref="/transitions" />
        <div className="border border-border">
          {transitionsLoading ? (
            <div className="px-3 py-8 text-center text-sm text-text-secondary">
              Loading transitions...
            </div>
          ) : activeTransitions.length === 0 ? (
            <div className="px-3 py-8 text-center">
              <Activity className="size-8 mx-auto mb-2 text-text-muted opacity-50" />
              <p className="text-sm font-medium text-text-primary">No active transitions</p>
              <p className="text-xs text-text-secondary mt-1">
                Devices are idle. Start a provisioning workflow to see activity here.
              </p>
            </div>
          ) : (
            <table className="w-full">
              <thead>
                <tr className="bg-bg-raised border-b border-border">
                  <th className="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide text-text-secondary">
                    Device
                  </th>
                  <th className="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide text-text-secondary">
                    Action
                  </th>
                  <th className="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide text-text-secondary">
                    Status
                  </th>
                  <th className="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide text-text-secondary">
                    Progress
                  </th>
                </tr>
              </thead>
              <tbody>
                {activeTransitions.map(({ device, transition }, idx) => (
                  <tr
                    key={`${device.uuid}-${transition.id}`}
                    className={`border-b border-border-muted last:border-0 ${
                      idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base"
                    } hover:bg-bg-raised transition-colors`}
                  >
                    <td className="px-3 py-2 text-xs text-text-primary">
                      <button
                        onClick={() => navigate(`/devices/${device.uuid}`)}
                        className="text-accent hover:text-accent-hover transition-colors cursor-pointer"
                      >
                        {getDeviceHostname(device)}
                      </button>
                    </td>
                    <td className="px-3 py-2">
                      <span className="text-xs text-status-transitioning font-medium">
                        {getTransitionAction(transition)}
                      </span>
                    </td>
                    <td className="px-3 py-2">
                      <StatusBadge status="transitioning" />
                    </td>
                    <td className="px-3 py-2">
                      <ProgressBar value={getTransitionProgress(transition)} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {/* Warnings */}
      <div>
        <SectionHeader title="Warnings" />
        {warningsLoading ? (
          <div className="border border-border px-3 py-8 text-center text-sm text-text-secondary">
            Loading warnings...
          </div>
        ) : warnings.length === 0 ? (
          <div className="border border-border px-3 py-8 text-center">
            <Server className="size-8 mx-auto mb-2 text-text-muted opacity-50" />
            <p className="text-sm font-medium text-text-primary">No warnings</p>
            <p className="text-xs text-text-secondary mt-1">
              All devices are operating normally.
            </p>
          </div>
        ) : (
          <div className="border border-border divide-y divide-border-muted">
            {warnings.map(({ device, warning }) => (
              <WarningRow
                key={`${device.uuid}-${warning.id}`}
                severity="warning"
                device={getDeviceHostname(device)}
                message={warning.message}
                onDismiss={() => handleDismissWarning(device, warning.id)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default Index;
