import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import type { DhcpLease, DhcpNetwork, StaticReservation, Device } from "@/lib/client";
import { createPendingDevice, makeLeaseStatic, getDevicesIndex } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Network, Eye, Pin } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface LeasesTableProps {
  network: DhcpNetwork;
  networkId: number;
  leases: DhcpLease[];
  onLeasesChange?: (leases: DhcpLease[]) => void;
  onReservationCreated?: (reservation: StaticReservation) => void;
}

export default function LeasesTable({ network, networkId, leases, onLeasesChange, onReservationCreated }: LeasesTableProps) {
  const navigate = useNavigate();
  const [isCreating, setIsCreating] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  // Static IP dialog state
  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedLease, setSelectedLease] = useState<DhcpLease | null>(null);
  const [customIp, setCustomIp] = useState("");
  const [hostname, setHostname] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Devices state for BMC lookup
  const [devices, setDevices] = useState<Device[]>([]);

  // Fetch all devices to identify BMC MACs
  useEffect(() => {
    const fetchDevices = async () => {
      try {
        const devicesIndex = await getDevicesIndex();
        setDevices(devicesIndex.devices);
      } catch (err) {
        console.error("Failed to fetch devices for BMC lookup:", err);
      }
    };

    fetchDevices();
  }, []);

  // Helper function to find device by BMC MAC
  const findDeviceByBmcMac = (mac: string): Device | undefined => {
    return devices.find(device =>
      device.attributes?.bmc?.mac_address?.toLowerCase() === mac.toLowerCase()
    );
  };

  const handleCreateDevice = async (lease: DhcpLease) => {
    setError(null);
    setSuccessMessage(null);
    setIsCreating(lease.mac_address);

    try {
      await createPendingDevice({
        mac_address: lease.mac_address,
        network_id: networkId,
      });

      setSuccessMessage(`Device creation initiated for ${lease.mac_address}. Waiting for machine to boot...`);

      // Auto-dismiss success message after 5 seconds
      setTimeout(() => {
        setSuccessMessage(null);
      }, 5000);

      // Refresh leases if callback is provided
      if (onLeasesChange) {
        onLeasesChange(leases);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create pending device");
    } finally {
      setIsCreating(null);
    }
  };

  const handleOpenStaticDialog = (lease: DhcpLease) => {
    setSelectedLease(lease);
    setCustomIp(lease.ip_address);
    setHostname(lease.hostname || "");
    setStaticDialogOpen(true);
  };

  const handleUseCurrentIp = () => {
    if (selectedLease) {
      setCustomIp(selectedLease.ip_address);
    }
  };

  const handleMakeStatic = async () => {
    if (!selectedLease) return;

    setError(null);
    setSuccessMessage(null);
    setIsSubmitting(true);

    try {
      const reservation = await makeLeaseStatic(selectedLease.id, {
        ip_address: customIp || undefined,
        hostname: hostname || undefined,
      });

      setSuccessMessage(
        `Static reservation created: ${reservation.ip_address} for ${reservation.mac_address}`
      );

      // Auto-dismiss success message after 5 seconds
      setTimeout(() => {
        setSuccessMessage(null);
      }, 5000);

      // Call the callback if provided
      if (onReservationCreated) {
        onReservationCreated(reservation);
      }

      // Close dialog
      setStaticDialogOpen(false);
      setSelectedLease(null);
      setCustomIp("");
      setHostname("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to make lease static");
    } finally {
      setIsSubmitting(false);
    }
  };

  const columns: ColumnDef<DhcpLease>[] = [
    {
      accessorKey: "mac_address",
      header: "MAC Address",
      cell: ({ row }) => {
        const mac = row.getValue("mac_address") as string;
        const bmcDevice = findDeviceByBmcMac(mac);

        return (
          <div className="flex items-center gap-2">
            <span className="font-mono text-xs">{mac}</span>
            {bmcDevice && (
              <Badge variant="secondary" className="text-xs">
                BMC
              </Badge>
            )}
          </div>
        );
      },
    },
    {
      accessorKey: "ip_address",
      header: "IP Address",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("ip_address")}</span>
      ),
    },
    {
      accessorKey: "device_uuid",
      header: "Device UUID",
      cell: ({ row }) => {
        const lease = row.original;
        const deviceUuid = lease.device_uuid;
        const bmcDevice = findDeviceByBmcMac(lease.mac_address);

        // If this MAC is a BMC, show the device it belongs to
        if (bmcDevice) {
          return (
            <div className="flex items-center gap-2">
              <button
                onClick={() => navigate(`/devices/${bmcDevice.uuid}`)}
                className="font-mono text-xs text-primary hover:underline"
              >
                {bmcDevice.uuid}
              </button>
            </div>
          );
        }

        // Otherwise show the normal device UUID if present
        if (deviceUuid) {
          return (
            <button
              onClick={() => navigate(`/devices/${deviceUuid}`)}
              className="font-mono text-xs text-primary hover:underline"
            >
              {deviceUuid}
            </button>
          );
        }

        return (
          <Badge variant="secondary" className="text-xs">
            No Device
          </Badge>
        );
      },
    },
    {
      accessorKey: "expires_at",
      header: "Expires At",
      cell: ({ row }) => {
        const expiresAt = row.getValue("expires_at") as string | undefined;
        if (expiresAt) {
          return <span className="text-sm">{new Date(expiresAt).toLocaleString()}</span>;
        }
        return <span className="text-muted-foreground text-sm">—</span>;
      },
    },
    {
      id: "actions",
      header: "Actions",
      cell: ({ row }) => {
        const lease = row.original;
        const deviceUuid = lease.device_uuid;
        const bmcDevice = findDeviceByBmcMac(lease.mac_address);

        return (
          <div className="flex gap-2">
            {bmcDevice ? (
              <Button
                variant="outline"
                size="sm"
                onClick={() => navigate(`/devices/${bmcDevice.uuid}`)}
                aria-label="View device"
              >
                <Eye className="h-4 w-4 mr-2" />
                View Device
              </Button>
            ) : deviceUuid ? (
              <Button
                variant="outline"
                size="sm"
                onClick={() => navigate(`/devices/${deviceUuid}`)}
                aria-label="View device"
              >
                <Eye className="h-4 w-4 mr-2" />
                View Device
              </Button>
            ) : (
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button
                    variant="default"
                    size="sm"
                    disabled={isCreating === lease.mac_address}
                    aria-label="Create device from lease"
                  >
                    {isCreating === lease.mac_address ? "Creating..." : "Create Device"}
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Create Device from Lease</AlertDialogTitle>
                    <AlertDialogDescription>
                      Marking this lease for device creation will wait for the machine to boot and provide
                      its UUID. Once the machine boots, device discovery will start automatically.
                      <div className="mt-4">
                        <span className="text-sm font-medium">MAC Address: </span>
                        <span className="font-mono text-sm">{lease.mac_address}</span>
                      </div>
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={() => handleCreateDevice(lease)}>
                      Create Device
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            )}

            <Button
              variant="outline"
              size="sm"
              onClick={() => handleOpenStaticDialog(lease)}
              aria-label="Make IP static"
            >
              <Pin className="h-4 w-4" />
            </Button>
          </div>
        );
      },
    },
  ];

  const table = useReactTable({
    data: leases,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <div className="space-y-4">
      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md text-sm">
          {error}
        </div>
      )}

      {successMessage && (
        <div className="bg-primary/10 border border-primary text-primary px-4 py-3 rounded-md text-sm">
          {successMessage}
        </div>
      )}

      <Dialog open={staticDialogOpen} onOpenChange={setStaticDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Make IP Address Static</DialogTitle>
            <DialogDescription>
              Assign a static IP address to this MAC address. You can use the current lease IP or specify a custom one.
            </DialogDescription>
          </DialogHeader>

          {selectedLease && (
            <div className="space-y-4">
              <div className="bg-muted p-3 rounded-md">
                <div className="text-sm">
                  <span className="font-medium">MAC Address: </span>
                  <span className="font-mono">{selectedLease.mac_address}</span>
                </div>
                <div className="text-sm mt-1">
                  <span className="font-medium">Current IP: </span>
                  <span className="font-mono">{selectedLease.ip_address}</span>
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="custom-ip">IP Address</Label>
                <div className="flex gap-2">
                  <Input
                    id="custom-ip"
                    value={customIp}
                    onChange={(e) => setCustomIp(e.target.value)}
                    placeholder="e.g., 192.168.1.100"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleUseCurrentIp}
                  >
                    Reset
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Subnet: {network.subnet}
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="hostname">Hostname (Optional)</Label>
                <Input
                  id="hostname"
                  value={hostname}
                  onChange={(e) => setHostname(e.target.value)}
                  placeholder="e.g., server-01"
                />
              </div>
            </div>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setStaticDialogOpen(false)}
              disabled={isSubmitting}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={handleMakeStatic}
              disabled={isSubmitting}
            >
              {isSubmitting ? "Creating..." : "Create Static Reservation"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="overflow-hidden rounded-md border">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  return (
                    <TableHead key={header.id}>
                      {header.isPlaceholder
                        ? null
                        : flexRender(header.column.columnDef.header, header.getContext())}
                    </TableHead>
                  );
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} data-state={row.getIsSelected() && "selected"}>
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={columns.length} className="h-24 text-center">
                  <div className="flex flex-col items-center gap-2">
                    <Network className="h-8 w-8 text-muted-foreground" />
                    <div className="text-muted-foreground">No active leases</div>
                  </div>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
