import { useState, useEffect } from "react";
import DevicesTableEnhanced from "@/components/devices/devices-table-enhanced";
import type { Device, DhcpLease, RoleWithOs, PendingDevice, Platform } from "@/lib/client";
import { useLoaderData, useRevalidator } from "react-router";
import { getRoles, deletePendingDevice } from "@/lib/client";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Clock, X } from "lucide-react";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";

function DevicesEnhanced() {
  const { devices: initialDevices, dhcpLeases: initialDhcpLeases, pendingDevices: initialPendingDevices, platforms: initialPlatforms } = useLoaderData() as {
    devices: Device[];
    dhcpLeases: DhcpLease[];
    pendingDevices: PendingDevice[];
    platforms: Platform[];
  };

  const revalidator = useRevalidator();
  const [dhcpLeases] = useState(initialDhcpLeases);
  const [roles, setRoles] = useState<RoleWithOs[]>([]);
  const [loading, setLoading] = useState(true);
  const [pendingDeviceToCancel, setPendingDeviceToCancel] = useState<number | null>(null);

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

    await deletePendingDevice(pendingDeviceToCancel);
    // Refresh the pending devices list
    revalidator.revalidate();
    setPendingDeviceToCancel(null);
  };

  // Create roles map for quick lookup
  const rolesMap = new Map(
    roles.map(role => [
      role.id!,
      { name: role.name, os_name: role.os_name, os_version: role.os_version }
    ])
  );

  // Create platforms map for quick lookup
  const platformsMap = new Map(
    initialPlatforms.map(p => [p.id!, { name: p.name }])
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
        platformsMap={platformsMap}
      />

      <DeleteConfirmationDialog
        open={pendingDeviceToCancel !== null}
        onOpenChange={(open) => !open && setPendingDeviceToCancel(null)}
        title="Cancel Pending Device"
        description="Are you sure you want to cancel this pending device registration? The DHCP lease will remain active."
        onConfirm={handleCancelPendingDevice}
      />
    </div>
  );
}

export default DevicesEnhanced;
