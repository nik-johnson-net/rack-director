import { useState, useEffect } from "react";
import DevicesTableEnhanced from "@/components/devices/devices-table-enhanced";
import type { Device, DhcpLease, RoleWithOs, PendingDevice } from "@/lib/client";
import { useLoaderData, useRevalidator } from "react-router";
import { getRoles, deletePendingDevice } from "@/lib/client";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Clock, X } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

function DevicesEnhanced() {
  const { devices: initialDevices, dhcpLeases: initialDhcpLeases, pendingDevices: initialPendingDevices } = useLoaderData() as {
    devices: Device[];
    dhcpLeases: DhcpLease[];
    pendingDevices: PendingDevice[];
  };

  const revalidator = useRevalidator();
  const [dhcpLeases] = useState(initialDhcpLeases);
  const [roles, setRoles] = useState<RoleWithOs[]>([]);
  const [loading, setLoading] = useState(true);
  const [pendingDeviceToCancel, setPendingDeviceToCancel] = useState<number | null>(null);
  const [isCancelling, setIsCancelling] = useState(false);

  useEffect(() => {
    const fetchData = async () => {
      try {
        const rolesData = await getRoles();
        setRoles(rolesData);
      } catch (error) {
        console.error("Failed to load additional data:", error);
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, []);

  const handleCancelPendingDevice = async () => {
    if (pendingDeviceToCancel === null) return;

    setIsCancelling(true);
    try {
      await deletePendingDevice(pendingDeviceToCancel);
      // Refresh the pending devices list
      revalidator.revalidate();
      setPendingDeviceToCancel(null);
    } catch (error) {
      console.error('Failed to cancel pending device:', error);
      alert('Failed to cancel pending device. Please try again.');
    } finally {
      setIsCancelling(false);
    }
  };

  // Create roles map for quick lookup
  const rolesMap = new Map(
    roles.map(role => [
      role.id!,
      { name: role.name, os_name: role.os_name, os_version: role.os_version }
    ])
  );

  if (loading) {
    return <div className="p-4">Loading device information...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="flex justify-between items-center">
        <h1 className="text-3xl font-bold">Devices</h1>
        <div className="text-sm text-muted-foreground">
          {initialDevices.length} device{initialDevices.length !== 1 ? 's' : ''}
        </div>
      </div>

      {initialPendingDevices.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Pending Devices</CardTitle>
            <CardDescription>Waiting for machines to boot and provide their UUID</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-2">
              {initialPendingDevices.map((pd) => (
                <div key={pd.id} className="flex items-center justify-between p-3 border rounded-md">
                  <div>
                    <div className="font-mono text-sm font-semibold">{pd.mac_address}</div>
                    <div className="text-xs text-muted-foreground">
                      Created {new Date(pd.created_at).toLocaleString()}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant="secondary">
                      <Clock className="h-3 w-3 mr-1" />
                      Awaiting Boot
                    </Badge>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setPendingDeviceToCancel(pd.id)}
                      className="text-red-600 hover:text-red-700 hover:bg-red-50"
                    >
                      <X className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      <DevicesTableEnhanced
        data={initialDevices}
        dhcpLeases={dhcpLeases}
        rolesMap={rolesMap}
      />

      <AlertDialog open={pendingDeviceToCancel !== null} onOpenChange={() => setPendingDeviceToCancel(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Cancel Pending Device</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to cancel this pending device registration? The DHCP lease will remain active.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isCancelling}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleCancelPendingDevice}
              disabled={isCancelling}
              className="bg-red-600 hover:bg-red-700"
            >
              {isCancelling ? 'Cancelling...' : 'Yes, Cancel'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

export default DevicesEnhanced;
