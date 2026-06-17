import { useState, useMemo } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { Monitor } from "lucide-react";
import type { Device, Platform, Role, PendingDevice } from "@/lib/client";
import { PageHeader } from "@/components/ui/page-header";
import { StatusBadge } from "@/components/ui/status-badge";
import { EmptyState } from "@/components/ui/empty-state";
import { Button } from "@/components/ui/button";

interface DevicesLoaderData {
  devices: Device[];
  platforms: Platform[];
  roles: Role[];
  pendingDevices: PendingDevice[];
}

function Devices() {
  const { devices, platforms, roles, pendingDevices } = useLoaderData() as DevicesLoaderData;
  const navigate = useNavigate();

  // Pending devices that haven't completed (not yet provisioned as real devices)
  const activePendingDevices = useMemo(
    () => pendingDevices.filter((p) => !p.completed_at),
    [pendingDevices]
  );

  const [search, setSearch] = useState("");
  const [lifecycleFilter, setLifecycleFilter] = useState("");
  const [platformFilter, setPlatformFilter] = useState("");
  const [roleFilter, setRoleFilter] = useState("");

  // Build lookup maps
  const platformsMap = useMemo(
    () => new Map(platforms.map((p) => [p.id!, p.name])),
    [platforms]
  );
  const rolesMap = useMemo(
    () => new Map(roles.map((r) => [r.id!, r.name])),
    [roles]
  );

  // Client-side filtering
  const filtered = useMemo(() => {
    return devices.filter((device) => {
      // Search: hostname, MAC, UUID
      if (search.trim()) {
        const q = search.trim().toLowerCase();
        const hostname = (device.attributes?.hostname ?? "").toLowerCase();
        // Match against network interface MACs
        const ifaceMacs = (device.attributes?.network_interfaces ?? [])
          .map((i) => i.mac_address.toLowerCase())
          .join(" ");
        const uuid = device.uuid.toLowerCase();
        if (
          !hostname.includes(q) &&
          !ifaceMacs.includes(q) &&
          !uuid.includes(q)
        ) {
          return false;
        }
      }

      // Lifecycle filter
      if (lifecycleFilter && device.lifecycle !== lifecycleFilter) {
        return false;
      }

      // Platform filter
      if (platformFilter) {
        const pid = platformFilter === "none" ? null : parseInt(platformFilter);
        if (pid === null) {
          if (device.platform_id != null) return false;
        } else {
          if (device.platform_id !== pid) return false;
        }
      }

      // Role filter
      if (roleFilter) {
        const rid = roleFilter === "none" ? null : parseInt(roleFilter);
        if (rid === null) {
          if (device.role_id != null) return false;
        } else {
          if (device.role_id !== rid) return false;
        }
      }

      return true;
    });
  }, [devices, search, lifecycleFilter, platformFilter, roleFilter]);

  const selectClass =
    "bg-bg-base border border-border text-text-primary text-xs px-3 py-1.5 rounded-sm focus:outline-none focus:border-accent appearance-none cursor-pointer pr-7";

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Devices" },
        ]}
        title="Devices"
        description={`${devices.length} device${devices.length !== 1 ? "s" : ""} registered${activePendingDevices.length > 0 ? `, ${activePendingDevices.length} pending` : ""}`}
        actions={
          <Button onClick={() => navigate("/devices/pending/new")}>
            + Add Pending Device
          </Button>
        }
      />

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-2 mb-4">
        {/* Search input */}
        <div className="relative">
          <input
            type="text"
            placeholder="Search hostname, MAC, UUID..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="bg-bg-base border border-border text-text-primary text-xs px-3 py-1.5 rounded-sm focus:outline-none focus:border-accent placeholder:text-text-muted"
            style={{ width: 260 }}
          />
        </div>

        {/* Lifecycle state */}
        <div className="relative">
          <select
            value={lifecycleFilter}
            onChange={(e) => setLifecycleFilter(e.target.value)}
            className={selectClass}
          >
            <option value="">All Lifecycle States</option>
            <option value="new">New</option>
            <option value="unprovisioned">Unprovisioned</option>
            <option value="provisioned">Provisioned</option>
            <option value="broken">Broken</option>
            <option value="removed">Removed</option>
          </select>
          <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-text-muted">
            <svg width="10" height="6" viewBox="0 0 10 6" fill="currentColor">
              <path d="M0 0l5 6 5-6z" />
            </svg>
          </span>
        </div>

        {/* Platform */}
        <div className="relative">
          <select
            value={platformFilter}
            onChange={(e) => setPlatformFilter(e.target.value)}
            className={selectClass}
          >
            <option value="">All Platforms</option>
            <option value="none">No Platform</option>
            {platforms.map((p) => (
              <option key={p.id} value={String(p.id)}>
                {p.name}
              </option>
            ))}
          </select>
          <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-text-muted">
            <svg width="10" height="6" viewBox="0 0 10 6" fill="currentColor">
              <path d="M0 0l5 6 5-6z" />
            </svg>
          </span>
        </div>

        {/* Role */}
        <div className="relative">
          <select
            value={roleFilter}
            onChange={(e) => setRoleFilter(e.target.value)}
            className={selectClass}
          >
            <option value="">All Roles</option>
            <option value="none">No Role</option>
            {roles.map((r) => (
              <option key={r.id} value={String(r.id)}>
                {r.name}
              </option>
            ))}
          </select>
          <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-text-muted">
            <svg width="10" height="6" viewBox="0 0 10 6" fill="currentColor">
              <path d="M0 0l5 6 5-6z" />
            </svg>
          </span>
        </div>
      </div>

      {/* Pending Devices section */}
      {activePendingDevices.length > 0 && (
        <div className="mb-4">
          <div className="text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2">
            Pending Devices ({activePendingDevices.length})
          </div>
          <div className="border border-border">
            <table className="w-full border-collapse">
              <thead>
                <tr className="bg-bg-raised">
                  {(["MAC Address", "Network", "Added", "Status"] as const).map((col) => (
                    <th
                      key={col}
                      className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                    >
                      {col}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {activePendingDevices.map((pending, idx) => {
                  const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                  return (
                    <tr
                      key={pending.id}
                      className={`${rowBg} border-b border-border-muted last:border-b-0`}
                    >
                      <td className="px-3 py-2 text-xs text-text-primary font-mono">
                        {pending.mac_address}
                      </td>
                      <td className="px-3 py-2 text-xs text-text-secondary">
                        Network #{pending.network_id}
                      </td>
                      <td className="px-3 py-2 text-xs text-text-secondary">
                        {new Date(pending.created_at).toLocaleString()}
                      </td>
                      <td className="px-3 py-2">
                        <StatusBadge status="new" />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Table */}
      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Hostname", "MAC", "Platform", "Role", "Lifecycle", "Actions"] as const).map(
                (col) => (
                  <th
                    key={col}
                    className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                  >
                    {col}
                  </th>
                )
              )}
            </tr>
          </thead>
          <tbody>
            {filtered.length === 0 ? (
              <tr>
                <td colSpan={6}>
                  <EmptyState
                    icon={Monitor}
                    title="No devices found"
                    description={
                      search || lifecycleFilter || platformFilter || roleFilter
                        ? "No devices match the current filters. Try adjusting your search."
                        : "No devices have been registered yet."
                    }
                  />
                </td>
              </tr>
            ) : (
              filtered.map((device, idx) => {
                const hostname = device.attributes?.hostname;
                const mac = device.attributes?.network_interfaces?.[0]?.mac_address;
                const platformName = device.platform_id
                  ? platformsMap.get(device.platform_id)
                  : undefined;
                const roleName = device.role_id
                  ? rolesMap.get(device.role_id)
                  : undefined;
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";

                return (
                  <tr
                    key={device.uuid}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Hostname */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {hostname ?? (
                        <span className="text-text-muted font-normal">—</span>
                      )}
                    </td>

                    {/* MAC */}
                    <td className="px-3 py-2 text-xs text-text-secondary font-mono">
                      {mac ?? <span className="text-text-muted">—</span>}
                    </td>

                    {/* Platform */}
                    <td className="px-3 py-2 text-xs text-text-primary">
                      {device.platform_id ? (
                        platformName ?? (
                          <span className="text-text-muted">Platform #{device.platform_id}</span>
                        )
                      ) : (
                        <span className="text-text-muted italic">detecting...</span>
                      )}
                    </td>

                    {/* Role */}
                    <td className="px-3 py-2 text-xs text-text-primary">
                      {device.role_id ? (
                        roleName ?? (
                          <span className="text-text-muted">Role #{device.role_id}</span>
                        )
                      ) : (
                        <span className="text-text-muted">—</span>
                      )}
                    </td>

                    {/* Lifecycle */}
                    <td className="px-3 py-2">
                      {device.lifecycle ? (
                        <StatusBadge status={device.lifecycle} />
                      ) : (
                        <span className="text-text-muted text-xs">—</span>
                      )}
                    </td>

                    {/* Actions */}
                    <td className="px-3 py-2">
                      <button
                        onClick={() => navigate(`/devices/${device.uuid}`)}
                        className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        aria-label={`View device ${hostname ?? device.uuid}`}
                      >
                        view
                      </button>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default Devices;
