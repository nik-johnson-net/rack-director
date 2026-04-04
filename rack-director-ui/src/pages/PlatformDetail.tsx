import { useState } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { PageHeader } from "@/components/ui/page-header";
import { Input } from "@/components/ui/input";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import {
  deletePlatform,
  updatePlatformDiskLabel,
  type Platform,
  type PlatformDisk,
  type PlatformDeviceInfo,
  type DeviceLifecycle,
} from "@/lib/client";
import { Pencil, Trash2, Server, Check, X } from "lucide-react";

interface LoaderData {
  platform: Platform;
  devices: PlatformDeviceInfo[];
}

// ── Inline disk label editor ──────────────────────────────────────────────────

interface DiskLabelCellProps {
  platformId: number;
  diskIndex: number;
  disk: PlatformDisk;
  onLabelChange: (index: number, newLabel: string | null) => void;
  onError: (msg: string) => void;
}

function DiskLabelCell({
  platformId,
  diskIndex,
  disk,
  onLabelChange,
  onError,
}: DiskLabelCellProps) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(disk.label ?? "");
  const [saving, setSaving] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);

  const handleEdit = () => {
    setValue(disk.label ?? "");
    setValidationError(null);
    setEditing(true);
  };

  const handleCancel = () => {
    setEditing(false);
    setValidationError(null);
  };

  const handleSave = async () => {
    setSaving(true);
    setValidationError(null);
    const newLabel = value.trim() || null;
    try {
      await updatePlatformDiskLabel(platformId, diskIndex, newLabel);
      onLabelChange(diskIndex, newLabel);
      setEditing(false);
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to update label";
      setValidationError(msg);
      onError(msg);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="flex items-center gap-1">
        <div className="space-y-1">
          <Input
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              setValidationError(null);
            }}
            placeholder="e.g., ROOT"
            className={`h-6 w-24 text-xs ${validationError ? "border-status-broken" : ""}`}
            disabled={saving}
            aria-label="Disk label"
            aria-invalid={!!validationError}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSave();
              if (e.key === "Escape") handleCancel();
            }}
            autoFocus
          />
          {validationError && (
            <p className="text-xs text-status-broken">{validationError}</p>
          )}
        </div>
        <button
          onClick={handleSave}
          disabled={saving}
          aria-label="Save label"
          className="p-0.5 text-accent hover:text-accent-hover disabled:opacity-50 cursor-pointer"
        >
          <Check className="h-3 w-3" />
        </button>
        <button
          onClick={handleCancel}
          disabled={saving}
          aria-label="Cancel edit"
          className="p-0.5 text-text-muted hover:text-text-secondary disabled:opacity-50 cursor-pointer"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-1 group">
      {disk.label ? (
        <span className="text-xs font-semibold text-accent">{disk.label}</span>
      ) : (
        <span className="text-text-muted text-xs">—</span>
      )}
      <button
        onClick={handleEdit}
        aria-label={`Edit label for disk ${diskIndex + 1}`}
        className="opacity-0 group-hover:opacity-100 transition-opacity p-0.5 text-text-muted hover:text-text-secondary cursor-pointer"
      >
        <Pencil className="h-3 w-3" />
      </button>
    </div>
  );
}

// ── Card wrapper ──────────────────────────────────────────────────────────────

function SectionCard({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border bg-bg-surface">
      <div className="px-4 py-3 border-b border-border">
        <span className="text-sm font-semibold text-text-primary">{title}</span>
      </div>
      <div className="px-4 py-4">{children}</div>
    </div>
  );
}

// ── KV row ────────────────────────────────────────────────────────────────────

function KVRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="grid gap-x-4 py-1" style={{ gridTemplateColumns: "140px 1fr" }}>
      <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">
        {label}
      </span>
      <span className="text-xs text-text-primary">{value}</span>
    </div>
  );
}

// ── Page component ────────────────────────────────────────────────────────────

function PlatformDetail() {
  const loaderData = useLoaderData<LoaderData>();
  const navigate = useNavigate();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [disks, setDisks] = useState<PlatformDisk[]>(
    loaderData.platform.attributes.disks
  );

  const platform = loaderData.platform;
  const devices = loaderData.devices;

  const handleDelete = async () => {
    try {
      await deletePlatform(platform.id!);
      navigate("/platforms");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete platform");
      setDeleteDialogOpen(false);
    }
  };

  const handleDiskLabelChange = (index: number, newLabel: string | null) => {
    setDisks((prev) =>
      prev.map((d, i) => (i === index ? { ...d, label: newLabel ?? undefined } : d))
    );
  };

  const cpus = platform.attributes.cpus;
  const nics = platform.attributes.nics;

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Platforms", href: "/platforms" },
          { label: platform.name },
        ]}
        title={platform.name}
        description={platform.description || "Hardware platform configuration"}
        actions={
          <div className="flex gap-2">
            <Button
              variant="secondary"
              onClick={() => navigate(`/platforms/${platform.id}/edit`)}
            >
              <Pencil className="h-3.5 w-3.5" />
              Edit
            </Button>
            <Button variant="danger" onClick={() => setDeleteDialogOpen(true)}>
              <Trash2 className="h-3.5 w-3.5" />
              Delete
            </Button>
          </div>
        }
      />

      {error && (
        <div className="mb-4 px-3 py-2 border border-error-border bg-error-bg text-status-broken text-xs">
          {error}
        </div>
      )}

      <div className="space-y-4" style={{ maxWidth: 900 }}>
        {/* Platform info KV */}
        <SectionCard title="Platform Info">
          <div className="space-y-0.5">
            <KVRow label="Name" value={platform.name} />
            <KVRow
              label="Description"
              value={
                platform.description ? (
                  platform.description
                ) : (
                  <span className="text-text-muted">—</span>
                )
              }
            />
            <KVRow
              label="Memory"
              value={
                platform.attributes.memory_gib > 0
                  ? `${platform.attributes.memory_gib} GiB`
                  : "—"
              }
            />
            <KVRow
              label="CPUs"
              value={
                cpus.length === 0 ? (
                  <span className="text-text-muted">—</span>
                ) : (
                  <div className="space-y-0.5">
                    {cpus.map((cpu, i) => (
                      <div key={i} className="text-xs text-text-primary">
                        {cpu.brand} {cpu.model}{" "}
                        <span className="text-text-secondary">({cpu.cores} cores)</span>
                      </div>
                    ))}
                  </div>
                )
              }
            />
            <KVRow
              label="NICs"
              value={
                nics.length === 0 ? (
                  <span className="text-text-muted">—</span>
                ) : (
                  <div className="space-y-0.5">
                    {nics.map((nic, i) => (
                      <div key={i} className="text-xs text-text-primary">
                        {nic.logical}
                        {nic.speed_gbps != null && (
                          <span className="text-text-secondary ml-2">
                            ({nic.speed_gbps} Gbps)
                          </span>
                        )}
                        {nic.label && (
                          <span className="ml-2 text-accent font-semibold">
                            {nic.label}
                          </span>
                        )}
                      </div>
                    ))}
                  </div>
                )
              }
            />
          </div>
        </SectionCard>

        {/* Disks table */}
        <SectionCard title="Disks">
          {disks.length === 0 ? (
            <p className="text-xs text-text-muted">No disks defined.</p>
          ) : (
            <table className="w-full border-collapse">
              <thead>
                <tr className="bg-bg-raised">
                  {["#", "Size", "Type", "Label"].map((col, i) => (
                    <th
                      key={i}
                      className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                    >
                      {col}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {disks.map((disk, index) => {
                  const rowBg = index % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                  return (
                    <tr
                      key={index}
                      className={`${rowBg} border-b border-border-muted last:border-b-0`}
                    >
                      <td className="px-3 py-2 text-xs text-text-muted">{index + 1}</td>
                      <td className="px-3 py-2 text-xs text-text-primary">
                        {disk.size_gb} GB
                      </td>
                      <td className="px-3 py-2 text-xs text-text-secondary uppercase">
                        {disk.disk_type}
                      </td>
                      <td className="px-3 py-2">
                        <DiskLabelCell
                          platformId={platform.id!}
                          diskIndex={index}
                          disk={disk}
                          onLabelChange={handleDiskLabelChange}
                          onError={(msg) => setError(msg)}
                        />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </SectionCard>

        {/* Assigned devices table */}
        <SectionCard title={`Assigned Devices (${devices.length})`}>
          {devices.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-8 text-center">
              <Server className="size-8 text-text-muted mb-3 opacity-50" />
              <p className="text-sm font-medium text-text-primary mb-1">
                No devices assigned
              </p>
              <p className="text-xs text-text-secondary">
                Devices will be automatically assigned when their hardware matches.
              </p>
            </div>
          ) : (
            <table className="w-full border-collapse">
              <thead>
                <tr className="bg-bg-raised">
                  {["UUID", "Hostname", "Lifecycle"].map((col, i) => (
                    <th
                      key={i}
                      className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                    >
                      {col}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {devices.map((device, idx) => {
                  const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                  return (
                    <tr
                      key={device.uuid}
                      className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                    >
                      <td className="px-3 py-2">
                        <button
                          onClick={() => navigate(`/devices/${device.uuid}`)}
                          className="text-xs font-mono text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        >
                          {device.uuid}
                        </button>
                      </td>
                      <td className="px-3 py-2 text-xs text-text-primary">
                        {device.hostname || (
                          <span className="text-text-muted">—</span>
                        )}
                      </td>
                      <td className="px-3 py-2">
                        {device.lifecycle ? (
                          <StatusBadge status={device.lifecycle as DeviceLifecycle} />
                        ) : (
                          <span className="text-text-muted text-xs">—</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </SectionCard>
      </div>

      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Platform?"
        description={
          devices.length > 0
            ? `This platform has ${devices.length} assigned device${
                devices.length !== 1 ? "s" : ""
              }. Deleting this platform will remove the platform assignment from these devices. This action cannot be undone.`
            : `Are you sure you want to delete the platform "${platform.name}"? This action cannot be undone.`
        }
        onConfirm={handleDelete}
      />
    </div>
  );
}

export default PlatformDetail;
