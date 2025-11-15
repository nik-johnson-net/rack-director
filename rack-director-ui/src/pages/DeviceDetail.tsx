import { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
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
  getDevice,
  getDeviceRole,
  getDeviceStatus,
  getDeviceTransitions,
  getDhcpLeaseByMac,
  getRoles,
  assignRoleToDevice,
  transitionDeviceLifecycle,
  type Device,
  type Role,
  type DeviceStatus,
  type LifecycleTransition,
  type DhcpLease,
  type RoleWithOs,
  type DeviceLifecycle,
} from "@/lib/client";
import { ArrowLeft, CheckCircle, Clock } from "lucide-react";

const getLifecycleColor = (lifecycle?: string) => {
  switch (lifecycle) {
    case "provisioned": return "bg-green-100 text-green-800 border-green-300";
    case "unprovisioned": return "bg-yellow-100 text-yellow-800 border-yellow-300";
    case "new": return "bg-blue-100 text-blue-800 border-blue-300";
    case "removed": return "bg-gray-100 text-gray-800 border-gray-300";
    case "broken": return "bg-red-100 text-red-800 border-red-300";
    default: return "bg-gray-100 text-gray-600 border-gray-300";
  }
};

const LIFECYCLE_STATES: DeviceLifecycle[] = ["new", "unprovisioned", "provisioned", "removed", "broken"];

function DeviceDetail() {
  const { uuid } = useParams<{ uuid: string }>();
  const navigate = useNavigate();

  const [device, setDevice] = useState<Device | null>(null);
  const [assignedRole, setAssignedRole] = useState<Role | null>(null);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [transitions, setTransitions] = useState<LifecycleTransition[]>([]);
  const [dhcpLease, setDhcpLease] = useState<DhcpLease | null>(null);
  const [availableRoles, setAvailableRoles] = useState<RoleWithOs[]>([]);
  const [selectedRoleId, setSelectedRoleId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [assigningRole, setAssigningRole] = useState(false);
  const [transitioning, setTransitioning] = useState(false);

  useEffect(() => {
    if (!uuid) return;

    const fetchData = async () => {
      try {
        const [deviceData, roleData, statusData, transitionsData, rolesData] = await Promise.all([
          getDevice(uuid),
          getDeviceRole(uuid),
          getDeviceStatus(uuid),
          getDeviceTransitions(uuid, true),
          getRoles()
        ]);

        setDevice(deviceData);
        setAssignedRole(roleData);
        setStatus(statusData);
        setTransitions(transitionsData);
        setAvailableRoles(rolesData);
        setSelectedRoleId(deviceData.role_id || null);

        // Fetch DHCP lease if MAC address is available
        if (deviceData.attributes?.mac_address) {
          const lease = await getDhcpLeaseByMac(deviceData.attributes.mac_address);
          setDhcpLease(lease);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load device data");
      } finally {
        setLoading(false);
      }
    };

    fetchData();
  }, [uuid]);

  const handleAssignRole = async () => {
    if (!uuid || !selectedRoleId) return;

    setAssigningRole(true);
    setError(null);

    try {
      await assignRoleToDevice(uuid, selectedRoleId);
      const updatedRole = await getDeviceRole(uuid);
      setAssignedRole(updatedRole);

      // Refresh device data
      const updatedDevice = await getDevice(uuid);
      setDevice(updatedDevice);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to assign role");
    } finally {
      setAssigningRole(false);
    }
  };

  const handleTransition = async (toState: DeviceLifecycle) => {
    if (!uuid) return;

    setTransitioning(true);
    setError(null);

    try {
      await transitionDeviceLifecycle(uuid, toState);

      // Refresh status and transitions
      const [updatedStatus, updatedTransitions] = await Promise.all([
        getDeviceStatus(uuid),
        getDeviceTransitions(uuid, true)
      ]);

      setStatus(updatedStatus);
      setTransitions(updatedTransitions);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to transition device");
    } finally {
      setTransitioning(false);
    }
  };

  if (loading) {
    return <div className="p-4">Loading device...</div>;
  }

  if (!device) {
    return <div className="p-4">Device not found</div>;
  }

  const ipAddress = device.attributes?.static_ip || dhcpLease?.ip_address;

  return (
    <div className="space-y-4 max-w-5xl">
      <div className="flex items-center gap-4">
        <Button
          variant="outline"
          size="icon"
          onClick={() => navigate('/devices')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex-1">
          <h1 className="text-3xl font-bold">
            {device.attributes?.hostname || device.uuid}
          </h1>
          <p className="text-sm text-gray-500 font-mono">{device.uuid}</p>
        </div>
        {status?.current_lifecycle && (
          <Badge variant="outline" className={`${getLifecycleColor(status.current_lifecycle)} text-sm px-3 py-1`}>
            {status.current_lifecycle}
          </Badge>
        )}
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}

      {/* Device Information */}
      <div className="grid grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Device Information</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <div className="grid grid-cols-2 gap-2 text-sm">
              <span className="font-medium">UUID:</span>
              <span className="font-mono text-xs">{device.uuid}</span>

              <span className="font-medium">Hostname:</span>
              <span>{device.attributes?.hostname || <span className="text-gray-400">—</span>}</span>

              <span className="font-medium">Architecture:</span>
              <Badge variant="outline" className="w-fit">{device.architecture}</Badge>

              <span className="font-medium">IP Address:</span>
              <span className="font-mono">{ipAddress || <span className="text-gray-400">—</span>}</span>

              <span className="font-medium">MAC Address:</span>
              <span className="font-mono text-xs">{device.attributes?.mac_address || <span className="text-gray-400">—</span>}</span>

              <span className="font-medium">Last Seen:</span>
              <span className="text-xs">{device.last_seen_at ? new Date(device.last_seen_at).toLocaleString() : "Never"}</span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Role Assignment</CardTitle>
            <CardDescription>
              Assign a provisioning role to this device
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {assignedRole && (
              <div className="p-3 bg-blue-50 rounded border border-blue-200">
                <div className="font-medium text-blue-900">{assignedRole.name}</div>
                <div className="text-xs text-blue-700 mt-1">
                  {assignedRole.description || "No description"}
                </div>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="role">Select Role</Label>
              <select
                id="role"
                value={selectedRoleId || ''}
                onChange={(e) => setSelectedRoleId(e.target.value ? parseInt(e.target.value) : null)}
                className="w-full border rounded-md px-3 py-2"
              >
                <option value="">No role</option>
                {availableRoles.map((role) => (
                  <option key={role.id} value={role.id}>
                    {role.name} ({role.os_name} {role.os_version})
                  </option>
                ))}
              </select>
            </div>

            <Button
              onClick={handleAssignRole}
              disabled={assigningRole || selectedRoleId === device.role_id}
              className="w-full"
            >
              {assigningRole ? "Assigning..." : "Assign Role"}
            </Button>
          </CardContent>
        </Card>
      </div>

      {/* Lifecycle Management */}
      <Card>
        <CardHeader>
          <CardTitle>Lifecycle Management</CardTitle>
          <CardDescription>
            Transition this device to a new lifecycle state
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            {LIFECYCLE_STATES.map((state) => (
              <AlertDialog key={state}>
                <AlertDialogTrigger asChild>
                  <Button
                    variant={status?.current_lifecycle === state ? "default" : "outline"}
                    disabled={transitioning || status?.current_lifecycle === state}
                    className="capitalize"
                  >
                    {state}
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Transition to {state}?</AlertDialogTitle>
                    <AlertDialogDescription>
                      This will transition the device from "{status?.current_lifecycle}" to "{state}".
                      A lifecycle transition plan will be created.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={() => handleTransition(state)}>
                      Transition
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* Transitions History */}
      <Card>
        <CardHeader>
          <CardTitle>Lifecycle Transitions</CardTitle>
          <CardDescription>
            History of all lifecycle state changes for this device
          </CardDescription>
        </CardHeader>
        <CardContent>
          {transitions.length === 0 ? (
            <p className="text-center text-gray-400 py-4">No transitions yet</p>
          ) : (
            <div className="space-y-2">
              {transitions.map((transition) => (
                <div
                  key={transition.id}
                  className="flex items-center gap-3 p-3 border rounded hover:bg-gray-50"
                >
                  {transition.completed_at ? (
                    <CheckCircle className="h-5 w-5 text-green-600 flex-shrink-0" />
                  ) : (
                    <Clock className="h-5 w-5 text-yellow-600 flex-shrink-0 animate-pulse" />
                  )}

                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <Badge variant="outline" className={getLifecycleColor(transition.from_state)}>
                        {transition.from_state}
                      </Badge>
                      <span className="text-gray-400">→</span>
                      <Badge variant="outline" className={getLifecycleColor(transition.to_state)}>
                        {transition.to_state}
                      </Badge>
                    </div>
                    <div className="text-xs text-gray-500 mt-1">
                      Started: {new Date(transition.started_at).toLocaleString()}
                      {transition.completed_at && (
                        <> • Completed: {new Date(transition.completed_at).toLocaleString()}</>
                      )}
                    </div>
                  </div>

                  {transition.plan_id && (
                    <Badge variant="secondary" className="text-xs">
                      Plan #{transition.plan_id}
                    </Badge>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Device Attributes (Raw JSON) */}
      <Card>
        <CardHeader>
          <CardTitle>Device Attributes</CardTitle>
          <CardDescription>
            Raw attribute data stored for this device
          </CardDescription>
        </CardHeader>
        <CardContent>
          <pre className="bg-gray-50 p-4 rounded text-xs font-mono overflow-x-auto">
            {JSON.stringify(device.attributes, null, 2)}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}

export default DeviceDetail;
