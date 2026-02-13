import { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/ui/status-badge";
import { TransitionStatusBadge } from "@/components/ui/transition-status-badge";
import { Breadcrumbs } from "@/components/ui/breadcrumbs";
import {
  getDevice,
  getDeviceRole,
  getDeviceStatus,
  getDeviceTransitions,
  getDhcpLeaseByMac,
  getRoles,
  assignRoleToDevice,
  transitionDeviceLifecycle,
  getStaticReservationByMac,
  makeLeaseStatic,
  getNetworks,
  deleteDevice,
  getDevicePlatform,
  getPlatforms,
  type Device,
  type Role,
  type DeviceStatus,
  type LifecycleTransition,
  type DhcpLease,
  type RoleWithOs,
  type DeviceLifecycle,
  type StaticReservation,
  type DhcpNetwork,
  type Platform,
} from "@/lib/client";
import { ArrowLeft, AlertCircle, Pin, Trash2 } from "lucide-react";
import { EditableHostname } from "@/components/devices/editable-hostname";
import { MakeStaticDialog } from "@/components/networks/make-static-dialog";
import { TransitionDialog } from "@/components/devices/transition-dialog";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { BmcConfiguration } from "@/components/devices/BmcConfiguration";
import { PlatformAssignment } from "@/components/devices/platform-assignment";
import { Label } from "@/components/ui/label";

const LIFECYCLE_STATES: DeviceLifecycle[] = ["new", "unprovisioned", "provisioned", "removed", "broken"];

function DeviceDetail() {
  const { uuid } = useParams<{ uuid: string }>();
  const navigate = useNavigate();

  const [device, setDevice] = useState<Device | null>(null);
  const [assignedRole, setAssignedRole] = useState<Role | null>(null);
  const [assignedPlatform, setAssignedPlatform] = useState<Platform | null>(null);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [transitions, setTransitions] = useState<LifecycleTransition[]>([]);
  const [dhcpLease, setDhcpLease] = useState<DhcpLease | null>(null);
  const [availableRoles, setAvailableRoles] = useState<RoleWithOs[]>([]);
  const [availablePlatforms, setAvailablePlatforms] = useState<Platform[]>([]);
  const [selectedRoleId, setSelectedRoleId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [assigningRole, setAssigningRole] = useState(false);
  const [transitioning, setTransitioning] = useState(false);

  // Static IP dialog state
  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedLease, setSelectedLease] = useState<DhcpLease | null>(null);
  const [selectedNetworkId, setSelectedNetworkId] = useState<number | null>(null);
  const [staticReservations, setStaticReservations] = useState<Map<string, StaticReservation>>(new Map());
  const [networks, setNetworks] = useState<DhcpNetwork[]>([]);

  // Transition dialog state
  const [transitionDialogOpen, setTransitionDialogOpen] = useState(false);
  const [targetState, setTargetState] = useState<DeviceLifecycle>("new");

  // Delete dialog state
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);

  useEffect(() => {
    if (!uuid) return;

    const fetchData = async () => {
      try {
        const [deviceData, roleData, platformData, statusData, transitionsData, rolesData, platformsData, networksData] = await Promise.all([
          getDevice(uuid),
          getDeviceRole(uuid),
          getDevicePlatform(uuid),
          getDeviceStatus(uuid),
          getDeviceTransitions(uuid, true),
          getRoles(),
          getPlatforms(),
          getNetworks()
        ]);

        setDevice(deviceData);
        setAssignedRole(roleData);
        setAssignedPlatform(platformData);
        setStatus(statusData);
        setTransitions(transitionsData);
        setAvailableRoles(rolesData);
        setAvailablePlatforms(platformsData);
        setSelectedRoleId(deviceData.role_id || null);
        setNetworks(networksData);

        // Fetch DHCP lease if MAC address is available
        if (deviceData.attributes?.mac_address) {
          const lease = await getDhcpLeaseByMac(deviceData.attributes.mac_address);
          setDhcpLease(lease);
        }

        // Fetch static reservations for all network interfaces
        if (deviceData.attributes?.network_interfaces) {
          const reservationsMap = new Map<string, StaticReservation>();

          for (const nic of deviceData.attributes.network_interfaces) {
            if (nic.network_id) {
              const reservation = await getStaticReservationByMac(nic.network_id, nic.mac_address);
              if (reservation) {
                reservationsMap.set(nic.mac_address, reservation);
              }
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

  const handleMakeStatic = async (ip: string) => {
    if (!selectedLease) return;

    setError(null);

    const reservation = await makeLeaseStatic(selectedLease.id, {
      ip_address: ip || undefined,
    });

    // Update static reservations map
    const updatedReservations = new Map(staticReservations);
    updatedReservations.set(selectedLease.mac_address, reservation);
    setStaticReservations(updatedReservations);

    // Reset selected lease
    setSelectedLease(null);
    setSelectedNetworkId(null);
  };

  const handleDeleteDevice = async () => {
    if (!uuid) return;

    setError(null);

    await deleteDevice(uuid);
    navigate('/devices');
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
      <Breadcrumbs
        items={[
          { label: "Devices", href: "/devices" },
          { label: device.attributes?.hostname || device.uuid },
        ]}
      />

      <div className="flex items-center gap-4">
        <Button
          variant="outline"
          size="icon"
          onClick={() => navigate('/devices')}
          aria-label="Back to devices"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex-1">
          <EditableHostname
            uuid={device.uuid}
            hostname={device.attributes?.hostname || device.uuid}
            onHostnameChange={async () => {
              // Refresh device data to get the updated hostname
              const updatedDevice = await getDevice(uuid!);
              setDevice(updatedDevice);
            }}
            onError={(errorMsg) => setError(errorMsg)}
          />
          <p className="text-sm text-muted-foreground font-mono">{device.uuid}</p>
        </div>
        {status?.current_lifecycle && (
          <StatusBadge status={status.current_lifecycle} className="text-sm px-3 py-1" />
        )}
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}

      {/* Static IP Dialog */}
      <MakeStaticDialog
        open={staticDialogOpen}
        onOpenChange={setStaticDialogOpen}
        lease={selectedLease}
        subnet={selectedNetworkId ? networks.find(n => n.id === selectedNetworkId)?.subnet : undefined}
        onConfirm={handleMakeStatic}
      />

      {/* Device Information */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
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

              <span className="font-medium">Network Interfaces:</span>
              <div className="col-span-2">
                {device.attributes?.network_interfaces && device.attributes.network_interfaces.length > 0 ? (
                  <table className="min-w-full text-sm border">
                    <thead>
                      <tr className="border-b bg-gray-50">
                        <th className="text-left py-2 px-3">Interface</th>
                        <th className="text-left py-2 px-3">MAC Address</th>
                        <th className="text-left py-2 px-3">IP Address</th>
                      </tr>
                    </thead>
                    <tbody>
                      {device.attributes.network_interfaces.map((nic, idx) => {
                        const hasStaticReservation = staticReservations.has(nic.mac_address);
                        return (
                          <tr
                            key={idx}
                            className={`border-b ${nic.disabled ? "bg-red-50 opacity-60" : ""}`}
                          >
                            <td className="py-2 px-3 font-mono text-xs">
                              {nic.interface_name}
                              {nic.disabled && (
                                <Badge className="ml-2 text-xs" variant="destructive">Disabled</Badge>
                              )}
                            </td>
                            <td className="py-2 px-3 font-mono text-xs">
                              {nic.mac_address}
                              {nic.warning_label && (
                                <div className="text-xs text-red-600 mt-1">
                                  ⚠️ {nic.warning_label}
                                </div>
                              )}
                            </td>
                            <td className="py-2 px-3 font-mono text-xs">
                              <div className="flex items-center gap-2">
                                {nic.ip_address || <span className="text-gray-400">—</span>}
                                {hasStaticReservation && (
                                  <Badge variant="secondary" className="text-xs">Static</Badge>
                                )}
                                {nic.ip_address && !hasStaticReservation && nic.network_id && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={async () => {
                                      // Look up the lease by MAC to get lease ID
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
                                    className="h-6 w-6 p-0"
                                  >
                                    <Pin className="h-3 w-3" />
                                  </Button>
                                )}
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                ) : (
                  <span className="text-gray-400">
                    {device.attributes?.mac_address ? (
                      <div className="text-sm">
                        <span className="font-mono">{device.attributes.mac_address}</span>
                        <span className="ml-2 text-xs text-gray-500">(Legacy format - trigger re-discovery to update)</span>
                      </div>
                    ) : (
                      "No network interfaces discovered"
                    )}
                  </span>
                )}
              </div>

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

        <PlatformAssignment
          uuid={uuid!}
          device={device}
          assignedPlatform={assignedPlatform}
          availablePlatforms={availablePlatforms}
          onPlatformUpdate={(updatedPlatform, updatedDevice) => {
            setAssignedPlatform(updatedPlatform);
            setDevice(updatedDevice);
          }}
          onError={(errorMsg) => setError(errorMsg)}
        />
      </div>

      {/* BMC Configuration */}
      <BmcConfiguration
        device={device}
        networks={networks}
        onDeviceUpdate={(updatedDevice) => setDevice(updatedDevice)}
        onError={(errorMsg) => setError(errorMsg)}
      />

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
              <Button
                key={state}
                variant={status?.current_lifecycle === state ? "default" : "outline"}
                disabled={transitioning || status?.current_lifecycle === state}
                className="capitalize"
                onClick={() => openTransitionDialog(state)}
              >
                {state}
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* Transition Dialog */}
      <TransitionDialog
        open={transitionDialogOpen}
        onOpenChange={setTransitionDialogOpen}
        currentState={status?.current_lifecycle ?? "new"}
        targetState={targetState}
        onConfirm={handleTransition}
      />

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
            <p className="text-center text-muted-foreground py-4">No transitions yet</p>
          ) : (
            <div className="space-y-3">
              {transitions.map((transition) => (
                <div
                  key={transition.id}
                  className="border rounded-lg p-4 space-y-2"
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <StatusBadge status={transition.from_state} />
                      <span className="text-muted-foreground">→</span>
                      <StatusBadge status={transition.to_state} />
                      <TransitionStatusBadge transition={transition} />
                    </div>
                    <span className="text-xs text-muted-foreground">
                      {new Date(transition.started_at).toLocaleString()}
                    </span>
                  </div>

                  {transition.plan_id && (
                    <Badge variant="secondary" className="text-xs">
                      Plan #{transition.plan_id}
                    </Badge>
                  )}

                  {transition.error_message && (
                    <div className="bg-destructive/10 border border-destructive/20 text-destructive rounded-md p-3 text-sm">
                      <div className="flex items-start gap-2">
                        <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
                        <div>
                          <p className="font-medium">Transition Failed</p>
                          <p className="mt-1 text-xs">{transition.error_message}</p>
                        </div>
                      </div>
                    </div>
                  )}

                  {transition.completed_at && (
                    <div className="text-xs text-muted-foreground">
                      Completed: {new Date(transition.completed_at).toLocaleString()}
                    </div>
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

      {/* Delete Device */}
      <Card className="border-red-200">
        <CardHeader>
          <CardTitle className="text-red-700">Danger Zone</CardTitle>
          <CardDescription>
            Irreversible and destructive actions
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button
            variant="destructive"
            className="w-full"
            onClick={() => setDeleteDialogOpen(true)}
          >
            <Trash2 className="h-4 w-4 mr-2" />
            Delete Device
          </Button>
        </CardContent>
      </Card>

      {/* Delete Device Dialog */}
      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Device?"
        description="This will permanently delete this device and all associated plans, transitions, and leases. This action cannot be undone."
        onConfirm={handleDeleteDevice}
      />
    </div>
  );
}

export default DeviceDetail;
