import { useState, useEffect } from "react";
import { useParams, useNavigate, Link } from "react-router";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { TransitionStatusBadge } from "@/components/ui/transition-status-badge";
import { PageHeader } from "@/components/ui/page-header";
import { KVGrid, KVRow } from "@/components/ui/kv-grid";
import { WarningRow } from "@/components/ui/warning-row";
import { cn } from "@/lib/utils";
import {
  ActionConsole,
  getDevice,
  getDeviceRole,
  getDeviceStatus,
  getDeviceTransitions,
  getDhcpLeaseByMac,
  getRoles,
  assignRoleToDevice,
  transitionDeviceLifecycle,
  cancelDeviceTransition,
  getStaticReservationByMac,
  makeLeaseStatic,
  getNetworks,
  deleteDevice,
  getDevicePlatform,
  getPlatforms,
  getDeviceWarnings,
  dismissDeviceWarning,
  postDevicePlan,
  type Action,
  type Device,
  type Role,
  type DeviceStatus,
  type LifecycleTransition,
  type DhcpLease,
  type DeviceLifecycle,
  type StaticReservation,
  type DhcpNetwork,
  type Platform,
  type DeviceWarning,
} from "@/lib/client";
import { AlertCircle, Pin, Trash2, Zap, XCircle, WrenchIcon } from "lucide-react";
import { EditableHostname } from "@/components/devices/editable-hostname";
import { MakeStaticDialog } from "@/components/networks/make-static-dialog";
import { TransitionDialog } from "@/components/devices/transition-dialog";
import { CancelTransitionDialog } from "@/components/devices/cancel-transition-dialog";
import { ProvisionDialog } from "@/components/devices/provision-dialog";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { BmcConfiguration } from "@/components/devices/BmcConfiguration";
import { PlatformAssignment } from "@/components/devices/platform-assignment";
import { DiskLabelOverrides } from "@/components/devices/disk-label-overrides";

type TabId = "overview" | "hardware" | "transitions" | "warnings";

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatRelativeTime(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

// ── Inline table component following design spec ──────────────────────────────

interface CardTableProps {
  headers: string[];
  rows: React.ReactNode[][];
  emptyMessage?: string;
}

function CardTable({ headers, rows, emptyMessage }: CardTableProps) {
  if (rows.length === 0) {
    return (
      <div className="py-6 text-center text-sm text-text-muted">
        {emptyMessage ?? "No data"}
      </div>
    );
  }
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="bg-bg-raised">
          {headers.map((h) => (
            <th
              key={h}
              className="px-3 py-2 text-left text-xs font-semibold text-text-secondary uppercase tracking-wide"
            >
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((cells, ri) => (
          <tr
            key={ri}
            className={cn(
              "border-t border-border-muted hover:bg-bg-raised transition-colors",
              ri % 2 === 1 ? "bg-bg-base" : "bg-bg-surface"
            )}
          >
            {cells.map((cell, ci) => (
              <td key={ci} className="px-3 py-2 text-text-primary">
                {cell}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ── Card wrapper with optional title ─────────────────────────────────────────

function SectionCard({
  title,
  children,
  className,
}: {
  title?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "bg-bg-surface border border-border",
        className
      )}
    >
      {title && (
        <div className="px-4 pt-4 pb-0">
          <h3 className="text-base font-semibold text-text-primary mb-3">{title}</h3>
        </div>
      )}
      <div className={cn(title ? "px-4 pb-4" : "p-4")}>{children}</div>
    </div>
  );
}

// ── Tab bar (client-side, no route change) ────────────────────────────────────

interface TabBarProps {
  tabs: { id: TabId; label: string; count?: number }[];
  active: TabId;
  onChange: (id: TabId) => void;
}

function TabBar({ tabs, active, onChange }: TabBarProps) {
  return (
    <div className="flex border-b border-border mb-6">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => onChange(tab.id)}
          className={cn(
            "px-4 py-2 text-sm transition-colors border-b-2 -mb-px cursor-pointer",
            active === tab.id
              ? "text-text-primary border-accent"
              : "text-text-secondary border-transparent hover:text-text-primary"
          )}
        >
          {tab.label}
          {tab.count !== undefined && tab.count > 0 && (
            <span className="ml-1.5 text-xs text-text-muted">({tab.count})</span>
          )}
        </button>
      ))}
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

function DeviceDetail() {
  const { uuid } = useParams<{ uuid: string }>();
  const navigate = useNavigate();

  const [device, setDevice] = useState<Device | null>(null);
  const [assignedRole, setAssignedRole] = useState<Role | null>(null);
  const [assignedPlatform, setAssignedPlatform] = useState<Platform | null>(null);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [transitions, setTransitions] = useState<LifecycleTransition[]>([]);
  const [warnings, setWarnings] = useState<DeviceWarning[]>([]);
  const [, setDhcpLease] = useState<DhcpLease | null>(null);
  const [availableRoles, setAvailableRoles] = useState<Role[]>([]);
  const [availablePlatforms, setAvailablePlatforms] = useState<Platform[]>([]);
  const [networks, setNetworks] = useState<DhcpNetwork[]>([]);
  const [staticReservations, setStaticReservations] = useState<Map<string, StaticReservation>>(new Map());

  const [loading, setLoading] = useState(true);
  const [info, setInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [transitioning, setTransitioning] = useState(false);
  const [dismissing, setDismissing] = useState<Set<number>>(new Set());

  const [activeTab, setActiveTab] = useState<TabId>("overview");

  // Dialog state
  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedLease, setSelectedLease] = useState<DhcpLease | null>(null);
  const [selectedNetworkId, setSelectedNetworkId] = useState<number | null>(null);
  const [transitionDialogOpen, setTransitionDialogOpen] = useState(false);
  const [targetState, setTargetState] = useState<DeviceLifecycle>("new");
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [provisionDialogOpen, setProvisionDialogOpen] = useState(false);
  const [cancelTransitionDialogOpen, setCancelTransitionDialogOpen] = useState(false);

  // Expandable error rows in transitions tab
  const [expandedTransitionIds, setExpandedTransitionIds] = useState<Set<number>>(new Set());

  useEffect(() => {
    if (!uuid) return;
    const fetchData = async () => {
      try {
        const [deviceData, roleData, platformData, statusData, transitionsData, warningsData, rolesData, platformsData, networksData] =
          await Promise.all([
            getDevice(uuid),
            getDeviceRole(uuid),
            getDevicePlatform(uuid),
            getDeviceStatus(uuid),
            getDeviceTransitions(uuid, true),
            getDeviceWarnings(uuid),
            getRoles(),
            getPlatforms(),
            getNetworks(),
          ]);

        setDevice(deviceData);
        setAssignedRole(roleData);
        setAssignedPlatform(platformData);
        setStatus(statusData);
        setTransitions(transitionsData);
        setWarnings(warningsData);
        setAvailableRoles(rolesData);
        setAvailablePlatforms(platformsData);
        setNetworks(networksData);

        if (deviceData.attributes?.mac_address) {
          const lease = await getDhcpLeaseByMac(deviceData.attributes.mac_address);
          setDhcpLease(lease);
        }

        if (deviceData.attributes?.network_interfaces) {
          const reservationsMap = new Map<string, StaticReservation>();
          for (const nic of deviceData.attributes.network_interfaces) {
            if (nic.network_id) {
              const reservation = await getStaticReservationByMac(nic.network_id, nic.mac_address);
              if (reservation) reservationsMap.set(nic.mac_address, reservation);
            }
          }
          setStaticReservations(reservationsMap);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load device data");
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, [uuid]);

  const openTransitionDialog = (toState: DeviceLifecycle) => {
    setTargetState(toState);
    setTransitionDialogOpen(true);
  };

  const handleTransition = async () => {
    if (!uuid) return;
    setTransitioning(true);
    setError(null);
    try {
      await transitionDeviceLifecycle(uuid, targetState);
      const [updatedStatus, updatedTransitions, updatedDevice] = await Promise.all([
        getDeviceStatus(uuid),
        getDeviceTransitions(uuid, true),
        getDevice(uuid),
      ]);
      setStatus(updatedStatus);
      setTransitions(updatedTransitions);
      setDevice(updatedDevice);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to transition device");
    } finally {
      setTransitioning(false);
    }
  };

  const handleProvision = async (roleId: number) => {
    if (!uuid) return;
    setTransitioning(true);
    setError(null);
    try {
      // Assign the role first, then transition to provisioned
      await assignRoleToDevice(uuid, roleId);
      await transitionDeviceLifecycle(uuid, "provisioned");
      const [updatedStatus, updatedTransitions, updatedRole, updatedDevice] = await Promise.all([
        getDeviceStatus(uuid),
        getDeviceTransitions(uuid, true),
        getDeviceRole(uuid),
        getDevice(uuid),
      ]);
      setStatus(updatedStatus);
      setTransitions(updatedTransitions);
      setAssignedRole(updatedRole);
      setDevice(updatedDevice);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to provision device");
      throw err;
    } finally {
      setTransitioning(false);
    }
  };

  const handleCancelTransition = async () => {
    if (!uuid) return;
    setTransitioning(true);
    setError(null);
    try {
      await cancelDeviceTransition(uuid);
      const [updatedStatus, updatedTransitions, updatedDevice] = await Promise.all([
        getDeviceStatus(uuid),
        getDeviceTransitions(uuid, true),
        getDevice(uuid),
      ]);
      setStatus(updatedStatus);
      setTransitions(updatedTransitions);
      setDevice(updatedDevice);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to cancel transition");
    } finally {
      setTransitioning(false);
    }
  };

  const handleMakeStatic = async (ip: string) => {
    if (!selectedLease) return;
    setError(null);
    const reservation = await makeLeaseStatic(selectedLease.id, { ip_address: ip || undefined });
    const updatedReservations = new Map(staticReservations);
    updatedReservations.set(selectedLease.mac_address, reservation);
    setStaticReservations(updatedReservations);
    setSelectedLease(null);
    setSelectedNetworkId(null);
  };

  const handleDeleteDevice = async () => {
    if (!uuid) return;
    setError(null);
    await deleteDevice(uuid);
    navigate("/devices");
  };

  const handleDismissWarning = async (warningId: number) => {
    if (!uuid) return;
    setDismissing((prev) => new Set(prev).add(warningId));
    try {
      await dismissDeviceWarning(uuid, warningId);
      setWarnings((prev) => prev.filter((w) => w.id !== warningId));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to dismiss warning");
    } finally {
      setDismissing((prev) => {
        const next = new Set(prev);
        next.delete(warningId);
        return next;
      });
    }
  };

  const createPlan = async (plan: Action[], msg: string) => {
    if (!uuid) return;
    try {
      await postDevicePlan(uuid, plan);
      setInfo("Requested device to " + msg);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create plan")
    }
  }

  // ── Guard renders ────────────────────────────────────────────────────────

  if (!uuid) return <div className="p-4 text-text-secondary">Invalid device URL</div>;
  if (loading) return <div className="p-4 text-text-secondary">Loading device...</div>;
  if (!device) return <div className="p-4 text-text-secondary">Device not found</div>;

  const hostname = device.attributes?.hostname || device.uuid;
  const lifecycle = status?.current_lifecycle ?? device.lifecycle ?? "new";
  const nics = device.attributes?.network_interfaces ?? [];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const disksAttr: any[] = device.attributes?.disks ?? [];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const cpusAttr: any[] = device.attributes?.cpus ?? [];
  const memoryGib: number | undefined = device.attributes?.memory_gib;
  const bootMode: string | undefined = device.attributes?.boot_mode;
  const recentTransitions = transitions.slice(0, 5);

  // Contextual action buttons based on lifecycle
  const isUnprovisioned = lifecycle === "unprovisioned";
  const isProvisioned = lifecycle === "provisioned";
  const isBroken = lifecycle === "broken";

  // ── Tab content ──────────────────────────────────────────────────────────

  const renderOverview = () => (
    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
      {/* Identity card */}
      <SectionCard title="Identity">
        <KVGrid>
          <KVRow label="UUID" value={<span className="text-xs font-mono break-all">{device.uuid}</span>} />
          <KVRow
            label="Hostname"
            value={
              <EditableHostname
                uuid={device.uuid}
                hostname={hostname}
                onHostnameChange={async () => {
                  const updated = await getDevice(uuid);
                  setDevice(updated);
                }}
                onError={(msg) => setError(msg)}
              />
            }
          />
          <KVRow label="Architecture" value={<span className="text-xs">{device.architecture}</span>} />
          <KVRow
            label="Boot Mode"
            value={
              bootMode ? (
                <span className="text-xs uppercase">{bootMode}</span>
              ) : (
                <span className="text-text-muted text-xs">—</span>
              )
            }
          />
          <KVRow
            label="Platform"
            value={
              assignedPlatform ? (
                <Link
                  to={`/platforms/${assignedPlatform.id}`}
                  className="text-accent hover:text-accent-hover text-xs transition-colors"
                >
                  {assignedPlatform.name}
                </Link>
              ) : (
                <span className="text-text-muted text-xs">None</span>
              )
            }
          />
          <KVRow
            label="Role"
            value={
              assignedRole ? (
                <Link
                  to={`/roles/${assignedRole.id}`}
                  className="text-accent hover:text-accent-hover text-xs transition-colors"
                >
                  {assignedRole.name}
                </Link>
              ) : (
                <span className="text-text-muted text-xs">None</span>
              )
            }
          />
          <KVRow
            label="Last Seen"
            value={
              <span className="text-xs">
                {device.last_seen_at ? new Date(device.last_seen_at).toLocaleString() : "Never"}
              </span>
            }
          />
        </KVGrid>
      </SectionCard>

      {/* Network Interfaces card */}
      <SectionCard title="Network Interfaces">
        <CardTable
          headers={["Interface", "MAC Address", "IP Address"]}
          emptyMessage="No network interfaces discovered"
          rows={nics.map((nic) => {
            const hasStatic = staticReservations.has(nic.mac_address);
            return [
              <span key="iface" className="text-xs font-mono">
                {nic.interface_name}
                {nic.disabled && (
                  <span className="ml-2 text-xs text-status-broken">[disabled]</span>
                )}
              </span>,
              <span key="mac" className="text-xs font-mono">
                {nic.mac_address}
                {nic.warning_label && (
                  <span className="block text-xs text-status-unprovisioned mt-0.5">
                    {nic.warning_label}
                  </span>
                )}
              </span>,
              <span key="ip" className="flex items-center gap-2 text-xs font-mono">
                {nic.ip_address ?? <span className="text-text-muted">—</span>}
                {hasStatic && (
                  <span className="text-xs text-accent">[static]</span>
                )}
                {nic.ip_address && !hasStatic && nic.network_id && (
                  <button
                    onClick={async () => {
                      const lease = await getDhcpLeaseByMac(nic.mac_address);
                      if (lease) {
                        setSelectedLease(lease);
                        setSelectedNetworkId(nic.network_id ?? null);
                        setStaticDialogOpen(true);
                      } else {
                        setError("No DHCP lease found for this interface");
                      }
                    }}
                    aria-label="Make IP static"
                    className="text-text-muted hover:text-accent transition-colors cursor-pointer"
                  >
                    <Pin className="size-3" />
                  </button>
                )}
              </span>,
            ];
          })}
        />
        {/* Legacy MAC fallback */}
        {nics.length === 0 && device.attributes?.mac_address && (
          <div className="mt-2 text-xs text-text-secondary font-mono">
            {device.attributes.mac_address}
            <span className="ml-2 text-text-muted">(legacy – re-scan to update)</span>
          </div>
        )}
      </SectionCard>

      {/* Disks card */}
      <SectionCard title="Disks">
        <CardTable
          headers={["Label", "Path", "Size", "Type"]}
          emptyMessage="No disk information available"
          rows={disksAttr.map((disk) => [
            disk.label ? (
              <span key="label" className="text-xs font-medium text-accent">{disk.label}</span>
            ) : (
              <span key="label" className="text-text-muted text-xs">—</span>
            ),
            <span key="path" className="text-xs font-mono">{disk.path ?? "—"}</span>,
            <span key="size" className="text-xs">{disk.size != null ? `${disk.size} GB` : "—"}</span>,
            <span key="type" className="text-xs uppercase text-text-secondary">{disk.disk_type ?? "—"}</span>,
          ])}
        />
      </SectionCard>

      {/* Recent Transitions card */}
      <SectionCard title="Recent Transitions">
        <CardTable
          headers={["Time", "From", "To", "Result"]}
          emptyMessage="No transitions yet"
          rows={recentTransitions.map((t) => [
            <span key="time" className="text-xs text-text-secondary">
              {formatRelativeTime(t.started_at)}
            </span>,
            <StatusBadge key="from" status={t.from_state} />,
            <StatusBadge key="to" status={t.to_state} />,
            <TransitionStatusBadge key="result" transition={t} />,
          ])}
        />
        {transitions.length > 5 && (
          <button
            onClick={() => setActiveTab("transitions")}
            className="mt-3 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
          >
            View all {transitions.length} transitions
          </button>
        )}
      </SectionCard>
    </div>
  );

  const renderHardware = () => (
    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
      {/* CPU */}
      <SectionCard title="CPU">
        {cpusAttr.length > 0 ? (
          <KVGrid>
            {cpusAttr.map((cpu, i: number) => (
              <>
                {cpusAttr.length > 1 && (
                  <KVRow
                    key={`cpu-label-${i}`}
                    label={`CPU ${i + 1}`}
                    value={<span className="text-xs text-text-muted">{cpu.brand} {cpu.model}</span>}
                  />
                )}
                {cpusAttr.length === 1 && (
                  <>
                    <KVRow key="brand" label="Brand" value={<span className="text-xs">{cpu.brand ?? "—"}</span>} />
                    <KVRow key="model" label="Model" value={<span className="text-xs">{cpu.model ?? "—"}</span>} />
                    <KVRow key="cores" label="Cores" value={<span className="text-xs">{cpu.cores ?? "—"}</span>} />
                  </>
                )}
              </>
            ))}
          </KVGrid>
        ) : (
          <p className="text-sm text-text-muted py-4 text-center">No CPU data available</p>
        )}
      </SectionCard>

      {/* Memory */}
      <SectionCard title="Memory">
        <KVGrid>
          <KVRow
            label="Total RAM"
            value={
              memoryGib != null ? (
                <span className="text-xs">{memoryGib} GiB</span>
              ) : (
                <span className="text-text-muted text-xs">—</span>
              )
            }
          />
        </KVGrid>
      </SectionCard>

      {/* Full Disks */}
      <SectionCard title="Disks" className="lg:col-span-2">
        <CardTable
          headers={["Label", "Path", "Size", "Type"]}
          emptyMessage="No disk information available"
          rows={disksAttr.map((disk) => [
            disk.label ? (
              <span key="label" className="text-xs font-medium text-accent">{disk.label}</span>
            ) : (
              <span key="label" className="text-text-muted text-xs">—</span>
            ),
            <span key="path" className="text-xs font-mono">{disk.path ?? "—"}</span>,
            <span key="size" className="text-xs">{disk.size != null ? `${disk.size} GB` : "—"}</span>,
            <span key="type" className="text-xs uppercase text-text-secondary">{disk.disk_type ?? "—"}</span>,
          ])}
        />
      </SectionCard>

      {/* BMC Configuration */}
      {(device.lifecycle === "new" || device.attributes?.bmc) && (
        <div className="lg:col-span-2">
          <BmcConfiguration
            device={device}
            networks={networks}
            onDeviceUpdate={(updated) => setDevice(updated)}
            onError={(msg) => setError(msg)}
          />
        </div>
      )}

      {/* Disk Label Overrides */}
      <div className="lg:col-span-2">
        <DiskLabelOverrides
          uuid={uuid}
          device={device}
          onDeviceUpdate={(updated) => setDevice(updated)}
          onError={(msg) => setError(msg)}
        />
      </div>

      {/* Platform Assignment */}
      <div className="lg:col-span-2">
        <PlatformAssignment
          uuid={uuid}
          device={device}
          assignedPlatform={assignedPlatform}
          availablePlatforms={availablePlatforms}
          onPlatformUpdate={(updatedPlatform, updatedDevice) => {
            setAssignedPlatform(updatedPlatform);
            setDevice(updatedDevice);
          }}
          onError={(msg) => setError(msg)}
        />
      </div>
    </div>
  );

  const renderTransitions = () => (
    <SectionCard>
      {transitions.length === 0 ? (
        <div className="py-8 text-center">
          <p className="text-sm text-text-muted">No transitions yet</p>
        </div>
      ) : (
        <table className="w-full text-sm">
          <thead>
            <tr className="bg-bg-raised">
              {["Time", "From", "To", "Plan", "Result", "Error"].map((h) => (
                <th
                  key={h}
                  className="px-3 py-2 text-left text-xs font-semibold text-text-secondary uppercase tracking-wide"
                >
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {transitions.map((t, ri) => {
              const isExpanded = expandedTransitionIds.has(t.id);
              const hasError = !!t.error_message;
              const toggleExpand = () => {
                setExpandedTransitionIds((prev) => {
                  const next = new Set(prev);
                  if (next.has(t.id)) next.delete(t.id);
                  else next.add(t.id);
                  return next;
                });
              };
              return (
                <>
                  <tr
                    key={t.id}
                    className={cn(
                      "border-t border-border-muted transition-colors",
                      ri % 2 === 1 ? "bg-bg-base" : "bg-bg-surface",
                      hasError ? "hover:bg-bg-raised cursor-pointer" : "hover:bg-bg-raised"
                    )}
                    onClick={hasError ? toggleExpand : undefined}
                  >
                    <td className="px-3 py-2 text-xs text-text-secondary whitespace-nowrap">
                      {new Date(t.started_at).toLocaleString()}
                    </td>
                    <td className="px-3 py-2">
                      <StatusBadge status={t.from_state} />
                    </td>
                    <td className="px-3 py-2">
                      <StatusBadge status={t.to_state} />
                    </td>
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {t.plan_id ? `#${t.plan_id}` : "—"}
                    </td>
                    <td className="px-3 py-2">
                      <TransitionStatusBadge transition={t} />
                    </td>
                    <td className="px-3 py-2 text-xs text-status-broken max-w-xs">
                      {hasError ? (
                        <span className={cn("block", !isExpanded && "truncate")}>
                          <span className="mr-1 text-text-muted">[{isExpanded ? "−" : "+"}]</span>
                          {t.error_message}
                        </span>
                      ) : (
                        "—"
                      )}
                    </td>
                  </tr>
                  {isExpanded && hasError && (
                    <tr
                      key={`${t.id}-expanded`}
                      className={cn(
                        "border-t border-border-muted",
                        ri % 2 === 1 ? "bg-bg-base" : "bg-bg-surface"
                      )}
                    >
                      <td colSpan={6} className="px-3 py-3">
                        <pre className="text-xs text-status-broken whitespace-pre-wrap break-all font-mono bg-bg-raised border border-border-muted px-3 py-2">
                          {t.error_message}
                        </pre>
                      </td>
                    </tr>
                  )}
                </>
              );
            })}
          </tbody>
        </table>
      )}
    </SectionCard>
  );

  const renderWarnings = () => (
    <div className="space-y-1">
      {warnings.length === 0 ? (
        <div className="py-8 text-center">
          <p className="text-sm text-text-muted">No active warnings</p>
        </div>
      ) : (
        warnings.map((w) => (
          <WarningRow
            key={w.id}
            severity="warning"
            message={`[${w.code}] ${w.message}`}
            onDismiss={dismissing.has(w.id) ? undefined : () => handleDismissWarning(w.id)}
          />
        ))
      )}
    </div>
  );

  // ── Danger zone ──────────────────────────────────────────────────────────

  const renderDangerZone = () => (
    <SectionCard>
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-semibold text-status-broken">Delete Device</p>
          <p className="text-xs text-text-secondary mt-0.5">
            Permanently delete this device and all associated data
          </p>
        </div>
        <Button
          variant="danger"
          size="sm"
          onClick={() => setDeleteDialogOpen(true)}
        >
          <Trash2 className="size-3.5" />
          Delete
        </Button>
      </div>
    </SectionCard>
  );

  // ── Render ───────────────────────────────────────────────────────────────

  const tabs: { id: TabId; label: string; count?: number }[] = [
    { id: "overview", label: "Overview" },
    { id: "hardware", label: "Hardware" },
    { id: "transitions", label: "Transitions", count: transitions.length },
    { id: "warnings", label: "Warnings", count: warnings.length },
  ];

  // Action buttons - contextual based on lifecycle
  const headerActions = (
    <div className="flex items-center gap-2">
      {/* Unprovisioned: Provision (asks for role) + Decommission */}
      {isUnprovisioned && (
        <>
          <Button
            variant="default"
            size="sm"
            onClick={() => setProvisionDialogOpen(true)}
            disabled={transitioning}
          >
            <Zap className="size-3.5" />
            Provision
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => openTransitionDialog("removed")}
            disabled={transitioning}
          >
            <XCircle className="size-3.5" />
            Decommission
          </Button>
        </>
      )}

      {/* Provisioned: Deprovision */}
      {isProvisioned && (
        <Button
          variant="danger"
          size="sm"
          onClick={() => openTransitionDialog("unprovisioned")}
          disabled={transitioning}
        >
          <XCircle className="size-3.5" />
          Deprovision
        </Button>
      )}

      {/* Broken: Deprovision + Provision (asks role) + Decommission */}
      {isBroken && (
        <>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => openTransitionDialog("unprovisioned")}
            disabled={transitioning}
          >
            <XCircle className="size-3.5" />
            Deprovision
          </Button>
          <Button
            variant="default"
            size="sm"
            onClick={() => setProvisionDialogOpen(true)}
            disabled={transitioning}
          >
            <Zap className="size-3.5" />
            Provision
          </Button>
          <Button
            variant="danger"
            size="sm"
            onClick={() => openTransitionDialog("removed")}
            disabled={transitioning}
          >
            <WrenchIcon className="size-3.5" />
            Decommission
          </Button>
        </>
      )}
      {/* Cancel Transition: shown when a transition is in progress */}
      {status?.active_transition && !status.active_transition.completed_at && (
        <Button
          variant="danger"
          size="sm"
          onClick={() => setCancelTransitionDialogOpen(true)}
          disabled={transitioning}
        >
          <XCircle className="size-3.5" />
          Cancel Transition
        </Button>
      )}
      <Button variant="danger" size="sm" onClick={() => createPlan([ActionConsole], "boot into console")} disabled={transitioning}><WrenchIcon className="size-3.5" />Console</Button>
    </div>
  );

  return (
    <div className="space-y-0">
      {/* Page header */}
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Devices", href: "/devices" },
          { label: hostname },
        ]}
        title={hostname}
        status={<StatusBadge status={lifecycle} />}
        description={[
          device.attributes?.mac_address ?? nics[0]?.mac_address,
          assignedPlatform?.name,
          assignedRole?.name,
        ]
          .filter(Boolean)
          .join(" · ")}
        actions={headerActions}
      />

      {/* Info banner */}
      {info && (
        <div className="mb-4 flex items-start gap-2 border border-error-border bg-error-bg px-3 py-2 text-sm text-status-broken">
          <AlertCircle className="size-4 shrink-0 mt-0.5" />
          <span>{info}</span>
          <button
            onClick={() => setError(null)}
            className="ml-auto text-text-muted hover:text-status-broken cursor-pointer"
            aria-label="Dismiss"
          >
            <XCircle className="size-4" />
          </button>
        </div>
      )}

      {/* Error banner */}
      {error && (
        <div className="mb-4 flex items-start gap-2 border border-error-border bg-error-bg px-3 py-2 text-sm text-status-broken">
          <AlertCircle className="size-4 shrink-0 mt-0.5" />
          <span>{error}</span>
          <button
            onClick={() => setError(null)}
            className="ml-auto text-text-muted hover:text-status-broken cursor-pointer"
            aria-label="Dismiss error"
          >
            <XCircle className="size-4" />
          </button>
        </div>
      )}

      {/* Tab bar */}
      <TabBar tabs={tabs} active={activeTab} onChange={setActiveTab} />

      {/* Tab panels */}
      {activeTab === "overview" && renderOverview()}
      {activeTab === "hardware" && renderHardware()}
      {activeTab === "transitions" && renderTransitions()}
      {activeTab === "warnings" && renderWarnings()}

      {/* Danger zone always visible at bottom */}
      <div className="pt-6">{renderDangerZone()}</div>

      {/* Dialogs */}
      <MakeStaticDialog
        open={staticDialogOpen}
        onOpenChange={setStaticDialogOpen}
        lease={selectedLease}
        subnet={selectedNetworkId ? networks.find((n) => n.id === selectedNetworkId)?.subnet : undefined}
        onConfirm={handleMakeStatic}
      />

      <TransitionDialog
        open={transitionDialogOpen}
        onOpenChange={setTransitionDialogOpen}
        currentState={status?.current_lifecycle ?? "new"}
        targetState={targetState}
        onConfirm={handleTransition}
      />

      <ProvisionDialog
        open={provisionDialogOpen}
        onOpenChange={setProvisionDialogOpen}
        availableRoles={availableRoles}
        currentRoleId={device?.role_id}
        onConfirm={handleProvision}
      />

      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Device?"
        description="This will permanently delete this device and all associated plans, transitions, and leases. This action cannot be undone."
        onConfirm={handleDeleteDevice}
      />

      <CancelTransitionDialog
        open={cancelTransitionDialogOpen}
        onOpenChange={setCancelTransitionDialogOpen}
        onConfirm={handleCancelTransition}
      />
    </div>
  );
}

export default DeviceDetail;
