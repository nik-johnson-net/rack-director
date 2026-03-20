import { useState } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/ui/status-badge";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Input } from "@/components/ui/input";
import { deletePlatform, updatePlatformDiskLabel, type Platform, type PlatformDisk, type PlatformDeviceInfo, type DeviceLifecycle } from "@/lib/client";
import { Pencil, Trash2, Server, Check, X } from "lucide-react";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";

interface LoaderData {
  platform: Platform;
  devices: PlatformDeviceInfo[];
}

// Inline label editor for a single platform disk row
interface DiskLabelCellProps {
  platformId: number;
  diskIndex: number;
  disk: PlatformDisk;
  onLabelChange: (index: number, newLabel: string | null) => void;
  onError: (msg: string) => void;
}

function DiskLabelCell({ platformId, diskIndex, disk, onLabelChange, onError }: DiskLabelCellProps) {
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
      // Show inline validation error for 422-style duplicate label errors
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
            className={`h-7 w-28 text-xs ${validationError ? "border-destructive" : ""}`}
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
            <p className="text-xs text-destructive">{validationError}</p>
          )}
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-primary"
          onClick={handleSave}
          disabled={saving}
          aria-label="Save label"
        >
          <Check className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground"
          onClick={handleCancel}
          disabled={saving}
          aria-label="Cancel edit"
        >
          <X className="h-3.5 w-3.5" />
        </Button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-1 group">
      {disk.label ? (
        <Badge variant="secondary" className="text-xs">{disk.label}</Badge>
      ) : (
        <span className="text-muted-foreground text-xs">—</span>
      )}
      <Button
        variant="ghost"
        size="icon"
        className="h-6 w-6 opacity-0 group-hover:opacity-100 transition-opacity"
        onClick={handleEdit}
        aria-label={`Edit label for disk ${diskIndex + 1}`}
      >
        <Pencil className="h-3 w-3" />
      </Button>
    </div>
  );
}

function PlatformDetail() {
  const loaderData = useLoaderData<LoaderData>();
  const navigate = useNavigate();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Local copy of disks so label edits reflect immediately without reload
  const [disks, setDisks] = useState<PlatformDisk[]>(loaderData.platform.attributes.disks);

  const platform = loaderData.platform;
  const devices = loaderData.devices;

  // Count devices by lifecycle state (excluding "removed")
  const deviceCounts = devices.reduce((acc, device) => {
    const lifecycle = device.lifecycle as DeviceLifecycle | undefined;
    if (lifecycle && lifecycle !== "removed") {
      acc[lifecycle] = (acc[lifecycle] || 0) + 1;
    }
    return acc;
  }, {} as Record<DeviceLifecycle, number>);

  const handleDelete = async () => {
    try {
      await deletePlatform(platform.id!);
      navigate('/platforms');
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

  const formatNicSummary = () => {
    const nics = platform.attributes.nics;
    if (nics.length === 0) return "No NICs";

    return nics.map((nic, index) => (
      <div key={index} className="text-sm">
        <span className="font-mono text-xs">{nic.logical}</span>
        {nic.speed_gbps && (
          <span className="text-muted-foreground ml-2">({nic.speed_gbps} Gbps)</span>
        )}
        {nic.label && <Badge variant="secondary" className="ml-2 text-xs">{nic.label}</Badge>}
      </div>
    ));
  };

  const formatCpuSummary = () => {
    const cpus = platform.attributes.cpus;
    if (cpus.length === 0) return "No CPUs";

    return cpus.map((cpu, index) => (
      <div key={index} className="text-sm">
        {cpu.brand} {cpu.model}
        <span className="text-muted-foreground ml-2">({cpu.cores} cores)</span>
      </div>
    ));
  };

  return (
    <div className="space-y-6 max-w-5xl">
      <PageHeader
        breadcrumbs={[
          { label: "Platforms", href: "/platforms" },
          { label: platform.name }
        ]}
        title={platform.name}
        description={platform.description || "Hardware platform configuration"}
        actions={
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => navigate(`/platforms/${platform.id}/edit`)}
            >
              <Pencil className="h-4 w-4 mr-2" />
              Edit
            </Button>
            <Button
              variant="destructive"
              onClick={() => setDeleteDialogOpen(true)}
            >
              <Trash2 className="h-4 w-4 mr-2" />
              Delete
            </Button>
          </div>
        }
      />

      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
          {error}
        </div>
      )}

      {/* Hardware Summary */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>CPUs</CardTitle>
            <CardDescription>Processor configuration</CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            {formatCpuSummary()}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Memory</CardTitle>
            <CardDescription>System memory</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{platform.attributes.memory_gib} GiB</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Disks</CardTitle>
            <CardDescription>Storage devices — click the pencil icon to edit a label</CardDescription>
          </CardHeader>
          <CardContent>
            {disks.length === 0 ? (
              <p className="text-sm text-muted-foreground">No disks</p>
            ) : (
              <table className="min-w-full text-sm">
                <thead>
                  <tr className="border-b">
                    <th className="text-left py-2 pr-4 font-medium text-xs text-muted-foreground">Path</th>
                    <th className="text-left py-2 pr-4 font-medium text-xs text-muted-foreground">Size</th>
                    <th className="text-left py-2 pr-4 font-medium text-xs text-muted-foreground">Type</th>
                    <th className="text-left py-2 font-medium text-xs text-muted-foreground">Label</th>
                  </tr>
                </thead>
                <tbody>
                  {disks.map((disk, index) => (
                    <tr key={index} className="border-b last:border-0">
                      <td className="py-2 pr-4 font-mono text-xs">{disk.path}</td>
                      <td className="py-2 pr-4 text-xs">{disk.size_gb} GB</td>
                      <td className="py-2 pr-4 text-xs uppercase">{disk.disk_type}</td>
                      <td className="py-2">
                        <DiskLabelCell
                          platformId={platform.id!}
                          diskIndex={index}
                          disk={disk}
                          onLabelChange={handleDiskLabelChange}
                          onError={(msg) => setError(msg)}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Network Interfaces</CardTitle>
            <CardDescription>NICs configuration</CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            {formatNicSummary()}
          </CardContent>
        </Card>
      </div>

      {/* Device Count by Lifecycle */}
      <Card>
        <CardHeader>
          <CardTitle>Assigned Devices</CardTitle>
          <CardDescription>
            Devices using this platform configuration
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            {Object.entries(deviceCounts).map(([lifecycle, count]) => (
              <div key={lifecycle} className="flex items-center gap-2">
                <StatusBadge status={lifecycle as DeviceLifecycle} />
                <span className="text-sm font-medium">{count}</span>
              </div>
            ))}
            {Object.keys(deviceCounts).length === 0 && (
              <span className="text-muted-foreground text-sm">No devices assigned</span>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Device List */}
      <Card>
        <CardHeader>
          <CardTitle>Device List</CardTitle>
          <CardDescription>
            All devices assigned to this platform
          </CardDescription>
        </CardHeader>
        <CardContent>
          {devices.length === 0 ? (
            <div className="text-center py-12">
              <Server className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
              <h3 className="text-lg font-semibold mb-2">No devices assigned</h3>
              <p className="text-muted-foreground">
                Devices will be automatically assigned to this platform when their hardware matches.
              </p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>UUID</TableHead>
                    <TableHead>Hostname</TableHead>
                    <TableHead>Lifecycle</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {devices.map((device) => (
                    <TableRow key={device.uuid}>
                      <TableCell>
                        <button
                          onClick={() => navigate(`/devices/${device.uuid}`)}
                          className="text-primary hover:underline font-mono text-xs"
                        >
                          {device.uuid}
                        </button>
                      </TableCell>
                      <TableCell>
                        {device.hostname || <span className="text-muted-foreground">—</span>}
                      </TableCell>
                      <TableCell>
                        {device.lifecycle ? (
                          <StatusBadge status={device.lifecycle as DeviceLifecycle} />
                        ) : (
                          <span className="text-muted-foreground">—</span>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Delete Confirmation Dialog */}
      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Platform?"
        description={
          devices.length > 0
            ? `This platform has ${devices.length} assigned device${devices.length !== 1 ? 's' : ''}. Deleting this platform will remove the platform assignment from these devices. This action cannot be undone.`
            : `Are you sure you want to delete the platform "${platform.name}"? This action cannot be undone.`
        }
        onConfirm={handleDelete}
      />
    </div>
  );
}

export default PlatformDetail;
