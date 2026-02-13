import { useState } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/ui/status-badge";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { deletePlatform, type Platform, type PlatformDeviceInfo, type DeviceLifecycle } from "@/lib/client";
import { Pencil, Trash2, Server } from "lucide-react";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";

interface LoaderData {
  platform: Platform;
  devices: PlatformDeviceInfo[];
}

function PlatformDetail() {
  const { platform, devices } = useLoaderData<LoaderData>();
  const navigate = useNavigate();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  const formatDiskSummary = () => {
    const disks = platform.attributes.disks;
    if (disks.length === 0) return "No disks";

    return disks.map((disk, index) => (
      <div key={index} className="text-sm">
        <span className="font-mono text-xs">{disk.path}</span>
        <span className="text-muted-foreground ml-2">
          ({disk.size_gb}GB, {disk.disk_type.toUpperCase()})
        </span>
        {disk.label && <Badge variant="secondary" className="ml-2 text-xs">{disk.label}</Badge>}
      </div>
    ));
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
            <CardDescription>Storage devices</CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            {formatDiskSummary()}
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
