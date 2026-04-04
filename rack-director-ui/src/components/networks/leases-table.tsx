import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import type { DhcpLease, DhcpNetwork, StaticReservation, Device, PendingDevice } from "@/lib/client";
import { createPendingDevice, makeLeaseStatic, getDevicesIndex } from "@/lib/client";
import { Network } from "lucide-react";
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

export default function LeasesTable({
  network,
  networkId,
  leases,
  pendingDevices,
  onLeasesChange,
  onReservationCreated,
}: LeasesTableProps) {
  const navigate = useNavigate();
  const [isCreating, setIsCreating] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedLease, setSelectedLease] = useState<DhcpLease | null>(null);

  const [devices, setDevices] = useState<Device[]>([]);

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

  const findDeviceByBmcMac = (mac: string): Device | undefined =>
    devices.find(
      (device) =>
        device.attributes?.bmc?.mac_address?.toLowerCase() === mac.toLowerCase()
    );

  const hasPendingDevice = (mac: string): boolean =>
    pendingDevices.some(
      (pd) => pd.mac_address.toLowerCase() === mac.toLowerCase() && !pd.completed_at
    );

  const handleCreateDevice = async (lease: DhcpLease) => {
    setError(null);
    setSuccessMessage(null);
    setIsCreating(lease.mac_address);

    try {
      await createPendingDevice({
        mac_address: lease.mac_address,
        network_id: networkId,
      });

      setSuccessMessage(
        `Device creation initiated for ${lease.mac_address}. Waiting for machine to boot...`
      );
      setTimeout(() => setSuccessMessage(null), 5000);

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
    setTimeout(() => setSuccessMessage(null), 5000);

    if (onReservationCreated) {
      onReservationCreated(reservation);
    }

    setSelectedLease(null);
  };

  return (
    <div className="space-y-3">
      {error && (
        <div className="px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
          {error}
        </div>
      )}
      {successMessage && (
        <div className="px-3 py-2 bg-status-provisioned-bg border-l-[3px] border-status-provisioned text-xs text-status-provisioned">
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

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["MAC Address", "IP Address", "Device", "Expires At", ""] as const).map((col, i) => (
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
            {leases.length === 0 ? (
              <tr>
                <td colSpan={5} className="py-8">
                  <div className="flex flex-col items-center gap-2 text-center">
                    <Network className="size-8 text-text-muted opacity-50" />
                    <p className="text-xs text-text-muted">No active leases</p>
                  </div>
                </td>
              </tr>
            ) : (
              leases.map((lease, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                const bmcDevice = findDeviceByBmcMac(lease.mac_address);
                const deviceUuid = lease.device_uuid;

                return (
                  <tr
                    key={lease.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* MAC Address */}
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-mono text-text-primary">
                          {lease.mac_address}
                        </span>
                        {bmcDevice && (
                          <span className="inline-flex items-center px-1.5 py-0.5 text-xs font-medium bg-status-new-bg text-status-new rounded-sm">
                            BMC
                          </span>
                        )}
                      </div>
                    </td>

                    {/* IP Address */}
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {lease.ip_address}
                    </td>

                    {/* Device UUID */}
                    <td className="px-3 py-2">
                      {bmcDevice ? (
                        <button
                          onClick={() => navigate(`/devices/${bmcDevice.uuid}`)}
                          className="text-xs font-mono text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        >
                          {bmcDevice.uuid}
                        </button>
                      ) : deviceUuid ? (
                        <button
                          onClick={() => navigate(`/devices/${deviceUuid}`)}
                          className="text-xs font-mono text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        >
                          {deviceUuid}
                        </button>
                      ) : (
                        <span className="text-xs text-text-muted">No Device</span>
                      )}
                    </td>

                    {/* Expires At */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {lease.lease_end
                        ? new Date(lease.lease_end).toLocaleString()
                        : <span className="text-text-muted">—</span>}
                    </td>

                    {/* Actions */}
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-3">
                        {bmcDevice ? (
                          <button
                            onClick={() => navigate(`/devices/${bmcDevice.uuid}`)}
                            className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                          >
                            view device
                          </button>
                        ) : deviceUuid ? (
                          <button
                            onClick={() => navigate(`/devices/${deviceUuid}`)}
                            className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                          >
                            view device
                          </button>
                        ) : (
                          <AlertDialog>
                            <AlertDialogTrigger asChild>
                              <button
                                type="button"
                                disabled={
                                  isCreating === lease.mac_address ||
                                  hasPendingDevice(lease.mac_address)
                                }
                                title={
                                  hasPendingDevice(lease.mac_address)
                                    ? "A pending device already exists for this MAC"
                                    : "Create a new device from this lease"
                                }
                                className="text-xs text-accent hover:text-accent-hover disabled:text-text-muted disabled:pointer-events-none transition-colors cursor-pointer"
                              >
                                {isCreating === lease.mac_address ? "creating..." : "create device"}
                              </button>
                            </AlertDialogTrigger>
                            <AlertDialogContent>
                              <AlertDialogHeader>
                                <AlertDialogTitle>Create Device from Lease</AlertDialogTitle>
                                <AlertDialogDescription>
                                  Marking this lease for device creation will wait for the machine to
                                  boot and provide its UUID. Once the machine boots, device discovery
                                  will start automatically.
                                  <div className="mt-4">
                                    <span className="font-medium">MAC Address: </span>
                                    <span className="font-mono">{lease.mac_address}</span>
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

                        <button
                          type="button"
                          onClick={() => handleOpenStaticDialog(lease)}
                          className="text-xs text-text-secondary hover:text-accent transition-colors cursor-pointer"
                          aria-label="Make IP static"
                        >
                          pin
                        </button>
                      </div>
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
