import { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/ui/status-badge";
import { Breadcrumbs } from "@/components/ui/breadcrumbs";
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
  getStaticReservationByMac,
  makeLeaseStatic,
  getNetworks,
  updateDeviceAttributes,
  deleteDevice,
  type Device,
  type Role,
  type DeviceStatus,
  type LifecycleTransition,
  type DhcpLease,
  type RoleWithOs,
  type DeviceLifecycle,
  type StaticReservation,
  type DhcpNetwork,
  type BmcConfig,
} from "@/lib/client";
import { ArrowLeft, CheckCircle, Clock, Pin, Trash2 } from "lucide-react";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

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

  // Static IP dialog state
  const [staticDialogOpen, setStaticDialogOpen] = useState(false);
  const [selectedInterface, setSelectedInterface] = useState<{
    mac: string;
    ip?: string;
    networkId?: number;
    leaseId?: number;
  } | null>(null);
  const [customIp, setCustomIp] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [staticReservations, setStaticReservations] = useState<Map<string, StaticReservation>>(new Map());
  const [networks, setNetworks] = useState<DhcpNetwork[]>([]);

  // BMC configuration state
  const [bmcMode, setBmcMode] = useState<"dhcp" | "static">("dhcp");
  const [bmcConfig, setBmcConfig] = useState<BmcConfig>({
    ip_address_source: "dhcp", // default to DHCP
    ip_address: "",
    netmask: "255.255.255.0",
    gateway: "",
    username: "ADMIN",
    password: "",
  });
  const [savingBmc, setSavingBmc] = useState(false);
  const [bmcConfigChanged, setBmcConfigChanged] = useState(false);

  useEffect(() => {
    if (!uuid) return;

    const fetchData = async () => {
      try {
        const [deviceData, roleData, statusData, transitionsData, rolesData, networksData] = await Promise.all([
          getDevice(uuid),
          getDeviceRole(uuid),
          getDeviceStatus(uuid),
          getDeviceTransitions(uuid, true),
          getRoles(),
          getNetworks()
        ]);

        setDevice(deviceData);
        setAssignedRole(roleData);
        setStatus(statusData);
        setTransitions(transitionsData);
        setAvailableRoles(rolesData);
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

        // Initialize BMC configuration from device attributes
        if (deviceData.attributes?.bmc_config) {
          // Use bmc_config if it exists
          setBmcConfig(deviceData.attributes.bmc_config);
          setBmcMode(deviceData.attributes.bmc_config.ip_address_source === "static" ? "static" : "dhcp");
        } else if (deviceData.attributes?.bmc) {
          // If BMC is discovered but no config exists, create default config
          const isDhcp = deviceData.attributes.bmc.ip_address_source.includes("DHCP");
          setBmcMode(isDhcp ? "dhcp" : "static");

          const network = networksData.find(n => n.id === deviceData.attributes.network_interfaces?.[0]?.network_id);
          setBmcConfig({
            ip_address_source: isDhcp ? "dhcp" : "static",
            ip_address: deviceData.attributes.bmc.ip_address || "",
            netmask: "255.255.255.0",
            gateway: network?.gateway || "",
            username: "ADMIN",
            password: "",
          });
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

  const handleSaveBmcConfig = async () => {
    if (!uuid || !device) return;

    setSavingBmc(true);
    setError(null);

    try {
      // Update device attributes with new BMC config
      // Always save a bmc_config object with ip_address_source
      let configToSave: BmcConfig;

      if (bmcMode === "dhcp") {
        // DHCP mode: save only ip_address_source, username, and password
        configToSave = {
          ip_address_source: "dhcp",
          username: bmcConfig.username,
          password: bmcConfig.password,
        };
      } else {
        // Static mode: save all fields
        configToSave = {
          ip_address_source: "static",
          ip_address: bmcConfig.ip_address,
          netmask: bmcConfig.netmask,
          gateway: bmcConfig.gateway,
          username: bmcConfig.username,
          password: bmcConfig.password,
        };
      }

      const updatedAttributes = {
        ...device.attributes,
        bmc_config: configToSave,
      };

      await updateDeviceAttributes(uuid, updatedAttributes);

      // Refresh device data
      const updatedDevice = await getDevice(uuid);
      setDevice(updatedDevice);
      setBmcConfigChanged(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save BMC configuration");
    } finally {
      setSavingBmc(false);
    }
  };

  const handleMakeStatic = async () => {
    if (!selectedInterface?.leaseId) return;

    setError(null);
    setIsSubmitting(true);

    try {
      const reservation = await makeLeaseStatic(selectedInterface.leaseId, {
        ip_address: customIp || undefined,
      });

      // Update static reservations map
      const updatedReservations = new Map(staticReservations);
      updatedReservations.set(selectedInterface.mac, reservation);
      setStaticReservations(updatedReservations);

      // Close dialog
      setStaticDialogOpen(false);
      setSelectedInterface(null);
      setCustomIp("");

      // Show success message
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to make lease static");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDeleteDevice = async () => {
    if (!uuid) return;

    setError(null);

    try {
      await deleteDevice(uuid);
      navigate('/devices');
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete device");
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
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex-1">
          <h1 className="text-3xl font-bold">
            {device.attributes?.hostname || device.uuid}
          </h1>
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
      <Dialog open={staticDialogOpen} onOpenChange={setStaticDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Make IP Address Static</DialogTitle>
            <DialogDescription>
              Assign a static IP address to this MAC address. You can use the current lease IP or specify a custom one.
            </DialogDescription>
          </DialogHeader>

          {selectedInterface && (
            <div className="space-y-4">
              <div className="bg-muted p-3 rounded-md">
                <div className="text-sm">
                  <span className="font-medium">MAC Address: </span>
                  <span className="font-mono">{selectedInterface.mac}</span>
                </div>
                <div className="text-sm mt-1">
                  <span className="font-medium">Current IP: </span>
                  <span className="font-mono">{selectedInterface.ip}</span>
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="custom-ip">IP Address</Label>
                <Input
                  id="custom-ip"
                  value={customIp}
                  onChange={(e) => setCustomIp(e.target.value)}
                  placeholder="e.g., 192.168.1.100"
                />
                {selectedInterface.networkId && networks.find(n => n.id === selectedInterface.networkId)?.subnet && (
                  <p className="text-xs text-muted-foreground">
                    Subnet: {networks.find(n => n.id === selectedInterface.networkId)?.subnet}
                  </p>
                )}
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
                            className={`border-b ${
                              nic.is_primary ? "bg-blue-50" : ""
                            } ${nic.disabled ? "bg-red-50 opacity-60" : ""}`}
                          >
                            <td className="py-2 px-3 font-mono text-xs">
                              {nic.interface_name}
                              {nic.is_primary && (
                                <Badge className="ml-2 text-xs" variant="default">Primary</Badge>
                              )}
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
                                        setSelectedInterface({
                                          mac: nic.mac_address,
                                          ip: nic.ip_address,
                                          networkId: nic.network_id,
                                          leaseId: lease.id
                                        });
                                        setCustomIp(nic.ip_address || "");
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
      </div>

      {/* BMC Configuration */}
      {device.attributes?.bmc && (
        <Card>
          <CardHeader>
            <CardTitle>BMC Configuration</CardTitle>
            <CardDescription>
              Baseboard Management Controller settings
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Discovered BMC Info */}
            <div className="p-3 bg-muted rounded-md border">
              <div className="text-sm font-medium mb-2">Discovered BMC</div>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-sm">
                <span className="text-muted-foreground">MAC Address:</span>
                <span className="font-mono">{device.attributes.bmc.mac_address}</span>

                <span className="text-muted-foreground">Current IP:</span>
                <span className="font-mono">{device.attributes.bmc.ip_address || "Not assigned"}</span>

                <span className="text-muted-foreground">IP Source:</span>
                <Badge variant={device.attributes.bmc.ip_address_source.includes("DHCP") ? "outline" : "secondary"}>
                  {device.attributes.bmc.ip_address_source}
                </Badge>
              </div>
            </div>

            {/* Pending Configuration Indicator */}
            {device.attributes.bmc_config &&
             device.attributes.bmc.ip_address_source &&
             ((device.attributes.bmc.ip_address_source.includes("DHCP") && device.attributes.bmc_config.ip_address_source === "static") ||
              (!device.attributes.bmc.ip_address_source.includes("DHCP") && device.attributes.bmc_config.ip_address_source === "dhcp")) && (
              <div className="p-3 bg-yellow-50 border border-yellow-200 rounded-md">
                <div className="flex items-center gap-2">
                  <Clock className="h-4 w-4 text-yellow-700" />
                  <div className="text-sm text-yellow-800">
                    <strong>Configuration Pending:</strong> BMC is currently using{" "}
                    <strong>{device.attributes.bmc.ip_address_source.includes("DHCP") ? "DHCP" : "Static IP"}</strong>,
                    but configured for{" "}
                    <strong>{device.attributes.bmc_config.ip_address_source === "dhcp" ? "DHCP" : "Static IP"}</strong>.
                    Changes will be applied on next discovery cycle.
                  </div>
                </div>
              </div>
            )}

            {/* BMC Configuration Mode Selector */}
            <Tabs value={bmcMode} onValueChange={(value) => {
              setBmcMode(value as "dhcp" | "static");
              setBmcConfigChanged(true);
            }}>
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="dhcp">DHCP</TabsTrigger>
                <TabsTrigger value="static">Static IP</TabsTrigger>
              </TabsList>

              <TabsContent value="dhcp" className="space-y-4">
                <div className="p-4 bg-muted rounded-md text-center">
                  <p className="text-sm text-muted-foreground">
                    BMC will use DHCP to obtain its IP address automatically.
                  </p>
                </div>

                <Button
                  onClick={handleSaveBmcConfig}
                  disabled={savingBmc || !bmcConfigChanged}
                  className="w-full"
                >
                  {savingBmc ? "Saving..." : "Save BMC Configuration"}
                </Button>
              </TabsContent>

              <TabsContent value="static" className="space-y-4">
                <div className="space-y-3">
                  <div className="space-y-2">
                    <Label htmlFor="bmc-ip">IP Address</Label>
                    <Input
                      id="bmc-ip"
                      type="text"
                      value={bmcConfig.ip_address}
                      onChange={(e) => {
                        setBmcConfig({ ...bmcConfig, ip_address: e.target.value });
                        setBmcConfigChanged(true);
                      }}
                      placeholder="e.g., 192.168.1.100"
                      className="font-mono"
                    />
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="bmc-netmask">Netmask</Label>
                    <Input
                      id="bmc-netmask"
                      type="text"
                      value={bmcConfig.netmask}
                      onChange={(e) => {
                        setBmcConfig({ ...bmcConfig, netmask: e.target.value });
                        setBmcConfigChanged(true);
                      }}
                      placeholder="e.g., 255.255.255.0"
                      className="font-mono"
                    />
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="bmc-gateway">Gateway</Label>
                    <Input
                      id="bmc-gateway"
                      type="text"
                      value={bmcConfig.gateway}
                      onChange={(e) => {
                        setBmcConfig({ ...bmcConfig, gateway: e.target.value });
                        setBmcConfigChanged(true);
                      }}
                      placeholder="e.g., 192.168.1.1"
                      className="font-mono"
                    />
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="bmc-password">Admin Password (optional)</Label>
                    <Input
                      id="bmc-password"
                      type="password"
                      value={bmcConfig.password || ""}
                      onChange={(e) => {
                        setBmcConfig({ ...bmcConfig, password: e.target.value });
                        setBmcConfigChanged(true);
                      }}
                      placeholder="Leave blank to keep current"
                    />
                  </div>

                  <Button
                    onClick={handleSaveBmcConfig}
                    disabled={savingBmc || !bmcConfigChanged}
                    className="w-full"
                  >
                    {savingBmc ? "Saving..." : "Save BMC Configuration"}
                  </Button>

                  {device.attributes.bmc_config && (
                    <div className="text-xs text-muted-foreground text-center">
                      BMC will be configured on next discovery cycle
                    </div>
                  )}
                </div>
              </TabsContent>
            </Tabs>
          </CardContent>
        </Card>
      )}

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
                      <StatusBadge status={transition.from_state} />
                      <span className="text-muted-foreground">→</span>
                      <StatusBadge status={transition.to_state} />
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

      {/* Delete Device */}
      <Card className="border-red-200">
        <CardHeader>
          <CardTitle className="text-red-700">Danger Zone</CardTitle>
          <CardDescription>
            Irreversible and destructive actions
          </CardDescription>
        </CardHeader>
        <CardContent>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="destructive" className="w-full">
                <Trash2 className="h-4 w-4 mr-2" />
                Delete Device
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Delete Device?</AlertDialogTitle>
                <AlertDialogDescription>
                  This will permanently delete this device and all associated plans, transitions, and leases. This action cannot be undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction onClick={handleDeleteDevice} className="bg-red-600 hover:bg-red-700">
                  Delete
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
    </div>
  );
}

export default DeviceDetail;
