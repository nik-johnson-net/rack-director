import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import type { DhcpLease, DhcpNetwork, StaticReservation, Device, PendingDevice } from "@/lib/client";
import { createPendingDevice, makeLeaseStatic, getDevicesIndex } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
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
import { MakeStaticDialog } from "./make-static-dialog";

interface LeasesTableProps {
  network: DhcpNetwork;
  networkId: number;
  leases: DhcpLease[];
  pendingDevices: PendingDevice[];
  onLeasesChange?: (leases: DhcpLease[]) => void;
  onReservationCreated?: (reservation: StaticReservation) => void;
}

export default function LeasesTable({ network, networkId, leases, pendingDevices, onLeasesChange, onReservationCreated }: LeasesTableProps) {
  const navigate = useNavigate();
  const [isCreating, setIsCreating] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  // Static IP dialog state
  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedLease, setSelectedLease] = useState<DhcpLease | null>(null);

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

  // Helper function to check if MAC has a pending device
  const hasPendingDevice = (mac: string): boolean => {
    return pendingDevices.some(
      pd => pd.mac_address.toLowerCase() === mac.toLowerCase() && !pd.completed_at
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
    setStaticDialogOpen(true);
  };

  const handleMakeStatic = async (ip: string, hostname?: string) => {
    if (!selectedLease) return;

    setError(null);
    setSuccessMessage(null);

    const reservation = await makeLeaseStatic(selectedLease.id, {
      ip_address: ip || undefined,
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

    // Reset selected lease
    setSelectedLease(null);
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
                    disabled={isCreating === lease.mac_address || hasPendingDevice(lease.mac_address)}
                    aria-label="Create device from lease"
                    title={hasPendingDevice(lease.mac_address) ? "A pending device already exists for this MAC" : "Create a new device from this lease"}
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

      <MakeStaticDialog
        open={staticDialogOpen}
        onOpenChange={setStaticDialogOpen}
        lease={selectedLease}
        subnet={network.subnet}
        onConfirm={handleMakeStatic}
      />

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
