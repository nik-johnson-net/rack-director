import { useState, useEffect } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
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
import { Clock, AlertCircle, Power, RotateCcw, Zap } from "lucide-react";
import {
  type Device,
  type DhcpNetwork,
  type BmcConfig,
  type PowerState,
  type PowerAction,
  updateDeviceAttributes,
  getDevice,
  getDevicePower,
  setDevicePower,
  ValidationError,
} from "@/lib/client";

type BmcConfigurationProps = {
  device: Device;
  networks: DhcpNetwork[];
  onDeviceUpdate: (device: Device) => void;
  onError: (error: string) => void;
};

// ── Power confirm dialog ─────────────────────────────────────────────────────

type PowerConfirmDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  action: "off" | "cycle";
  onConfirm: () => Promise<void>;
};

function PowerConfirmDialog({ open, onOpenChange, action, onConfirm }: PowerConfirmDialogProps) {
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleConfirm = async () => {
    setIsSubmitting(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {action === "off" ? "Power Off Device?" : "Power Cycle Device?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {action === "off"
              ? "This will send a hard power-off command to the BMC. The device will lose power immediately."
              : "This will send a power-cycle command to the BMC. The device will reboot immediately."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isSubmitting}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={isSubmitting}
            className="bg-destructive hover:bg-destructive/90 text-destructive-foreground"
          >
            {isSubmitting
              ? "Sending..."
              : action === "off"
              ? "Power Off"
              : "Power Cycle"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

// ── Power state badge ────────────────────────────────────────────────────────

function PowerStateBadge({ state, loading }: { state: PowerState | null; loading: boolean }) {
  if (loading) {
    return (
      <Badge variant="outline" className="text-text-muted">
        Checking...
      </Badge>
    );
  }
  if (state === "on") {
    return (
      <Badge variant="status-provisioned">
        <Power className="size-3" />
        On
      </Badge>
    );
  }
  if (state === "off") {
    return (
      <Badge variant="secondary">
        <Power className="size-3" />
        Off
      </Badge>
    );
  }
  // unknown or null
  return (
    <Badge variant="outline" className="text-text-muted">
      <Power className="size-3" />
      Unknown
    </Badge>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function BmcConfiguration({ device, networks, onDeviceUpdate, onError }: BmcConfigurationProps) {
  const [bmcMode, setBmcMode] = useState<"dhcp" | "static">("dhcp");
  const [bmcConfig, setBmcConfig] = useState<BmcConfig>({
    ip_address_source: "dhcp", // default to DHCP
    ip_address: "",
    netmask: "255.255.255.0",
    gateway: "",
  });
  const [savingBmc, setSavingBmc] = useState(false);
  const [bmcConfigChanged, setBmcConfigChanged] = useState(false);
  const [validationErrors, setValidationErrors] = useState<Record<string, string>>({});

  // Power state — lazy, non-blocking, never gates card render
  const [powerState, setPowerState] = useState<PowerState | null>(null);
  const [powerDriver, setPowerDriver] = useState<string | null>(null);
  const [powerLoading, setPowerLoading] = useState(false);
  const [powerActionInFlight, setPowerActionInFlight] = useState(false);

  // Confirmation dialog state
  const [confirmDialogOpen, setConfirmDialogOpen] = useState(false);
  const [pendingAction, setPendingAction] = useState<"off" | "cycle" | null>(null);

  useEffect(() => {
    // Initialize BMC configuration from device attributes
    if (device.attributes?.bmc_config) {
      // Use bmc_config if it exists
      setBmcConfig(device.attributes.bmc_config);
      setBmcMode(device.attributes.bmc_config.ip_address_source === "static" ? "static" : "dhcp");
    } else if (device.attributes?.bmc) {
      // If BMC is discovered but no config exists, create default config
      const isDhcp = device.attributes.bmc.ip_address_source.includes("DHCP");
      setBmcMode(isDhcp ? "dhcp" : "static");

      const network = networks.find(n => n.id === device.attributes.network_interfaces?.[0]?.network_id);
      setBmcConfig({
        ip_address_source: isDhcp ? "dhcp" : "static",
        ip_address: device.attributes.bmc.ip_address || "",
        netmask: "255.255.255.0",
        gateway: network?.gateway || "",
      });
    }
  }, [device, networks]);

  // Fetch power state lazily when a BMC is present — never blocks the card render
  useEffect(() => {
    if (!device.attributes?.bmc) return;
    let cancelled = false;

    setPowerLoading(true);
    getDevicePower(device.uuid).then((status) => {
      if (cancelled) return;
      setPowerState(status.state);
      setPowerDriver(status.driver);
    }).catch(() => {
      if (cancelled) return;
      setPowerState("unknown");
      setPowerDriver(null);
    }).finally(() => {
      if (!cancelled) setPowerLoading(false);
    });

    return () => { cancelled = true; };
  }, [device.uuid, device.attributes?.bmc]);

  const fetchPowerState = () => {
    setPowerLoading(true);
    getDevicePower(device.uuid).then((status) => {
      setPowerState(status.state);
      setPowerDriver(status.driver);
    }).catch(() => {
      setPowerState("unknown");
      setPowerDriver(null);
    }).finally(() => {
      setPowerLoading(false);
    });
  };

  const executePowerAction = async (action: PowerAction) => {
    setPowerActionInFlight(true);
    onError("");
    try {
      await setDevicePower(device.uuid, action);
      // Re-fetch state after action; state may take a moment to change but a single fetch is fine
      fetchPowerState();
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to execute power action");
    } finally {
      setPowerActionInFlight(false);
    }
  };

  const handlePowerOn = () => {
    executePowerAction("on");
  };

  const handlePowerOff = async () => {
    await executePowerAction("off");
  };

  const handlePowerCycle = async () => {
    await executePowerAction("cycle");
  };

  const requestDestructiveAction = (action: "off" | "cycle") => {
    setPendingAction(action);
    setConfirmDialogOpen(true);
  };

  const handleConfirmDestructive = async () => {
    if (!pendingAction) return;
    if (pendingAction === "off") {
      await handlePowerOff();
    } else {
      await handlePowerCycle();
    }
  };

  const handleSaveBmcConfig = async () => {
    setSavingBmc(true);
    onError("");
    setValidationErrors({});

    try {
      // Update device attributes with new BMC config
      // Always save a bmc_config object with ip_address_source
      let configToSave: BmcConfig;

      if (bmcMode === "dhcp") {
        // DHCP mode: save only ip_address_source
        configToSave = {
          ip_address_source: "dhcp",
        };
      } else {
        // Static mode: save ip_address_source and network settings
        configToSave = {
          ip_address_source: "static",
          ip_address: bmcConfig.ip_address,
          netmask: bmcConfig.netmask,
          gateway: bmcConfig.gateway,
        };
      }

      const updatedAttributes = {
        ...device.attributes,
        bmc_config: configToSave,
      };

      await updateDeviceAttributes(device.uuid, updatedAttributes);

      // Refresh device data
      const updatedDevice = await getDevice(device.uuid);
      onDeviceUpdate(updatedDevice);
      setBmcConfigChanged(false);
    } catch (err) {
      if (err instanceof ValidationError) {
        // Display field-specific validation errors
        setValidationErrors(err.errors);
        onError("Please fix the validation errors below");
      } else {
        onError(err instanceof Error ? err.message : "Failed to save BMC configuration");
      }
    } finally {
      setSavingBmc(false);
    }
  };

  // Only show if device is in "new" lifecycle or BMC is discovered
  if (device.lifecycle !== "new" && !device.attributes?.bmc) {
    return null;
  }

  const buttonsDisabled = powerActionInFlight || powerLoading;

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>BMC Configuration</CardTitle>
          <CardDescription>
            Baseboard Management Controller settings
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Discovered BMC Info */}
          {device.attributes?.bmc && (
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

                <span className="text-muted-foreground">Power State:</span>
                <div className="flex items-center gap-2">
                  <PowerStateBadge state={powerState} loading={powerLoading} />
                  {!powerLoading && powerDriver && (
                    <span className="text-xs text-text-muted">{powerDriver}</span>
                  )}
                </div>
              </div>

              {/* Power Controls */}
              <div className="mt-3 pt-3 border-t border-border-muted">
                <div className="text-xs text-text-secondary uppercase tracking-wide mb-2">Power Controls</div>
                <div className="flex items-center gap-2 flex-wrap">
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={handlePowerOn}
                    disabled={buttonsDisabled}
                    aria-label="Power on device"
                  >
                    <Zap className="size-3.5" />
                    Power On
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => requestDestructiveAction("off")}
                    disabled={buttonsDisabled}
                    aria-label="Power off device"
                  >
                    <Power className="size-3.5" />
                    Power Off
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => requestDestructiveAction("cycle")}
                    disabled={buttonsDisabled}
                    aria-label="Power cycle device"
                  >
                    <RotateCcw className="size-3.5" />
                    Power Cycle
                  </Button>
                </div>
              </div>
            </div>
          )}

          {/* Pending Configuration Indicator */}
          {device.attributes.bmc_config &&
           device.attributes.bmc?.ip_address_source &&
           ((device.attributes.bmc.ip_address_source.includes("DHCP") && device.attributes.bmc_config.ip_address_source === "static") ||
            (!device.attributes.bmc.ip_address_source.includes("DHCP") && device.attributes.bmc_config.ip_address_source === "dhcp")) && (
            <div className="p-3 bg-warn-bg border border-warn-border rounded-md">
              <div className="flex items-center gap-2">
                <Clock className="h-4 w-4 text-status-unprovisioned" />
                <div className="text-sm text-text-primary">
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
                      // Clear validation error when user starts typing
                      if (validationErrors["bmc_config.ip_address"]) {
                        const newErrors = { ...validationErrors };
                        delete newErrors["bmc_config.ip_address"];
                        setValidationErrors(newErrors);
                      }
                    }}
                    placeholder="e.g., 192.168.1.100"
                    className={`font-mono ${validationErrors["bmc_config.ip_address"] ? "border-destructive" : ""}`}
                  />
                  {validationErrors["bmc_config.ip_address"] && (
                    <div className="flex items-center gap-2 text-sm text-destructive">
                      <AlertCircle className="h-4 w-4" />
                      <span>{validationErrors["bmc_config.ip_address"]}</span>
                    </div>
                  )}
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
                      // Clear validation error when user starts typing
                      if (validationErrors["bmc_config.netmask"]) {
                        const newErrors = { ...validationErrors };
                        delete newErrors["bmc_config.netmask"];
                        setValidationErrors(newErrors);
                      }
                    }}
                    placeholder="e.g., 255.255.255.0"
                    className={`font-mono ${validationErrors["bmc_config.netmask"] ? "border-destructive" : ""}`}
                  />
                  {validationErrors["bmc_config.netmask"] && (
                    <div className="flex items-center gap-2 text-sm text-destructive">
                      <AlertCircle className="h-4 w-4" />
                      <span>{validationErrors["bmc_config.netmask"]}</span>
                    </div>
                  )}
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
                      // Clear validation error when user starts typing
                      if (validationErrors["bmc_config.gateway"]) {
                        const newErrors = { ...validationErrors };
                        delete newErrors["bmc_config.gateway"];
                        setValidationErrors(newErrors);
                      }
                    }}
                    placeholder="e.g., 192.168.1.1"
                    className={`font-mono ${validationErrors["bmc_config.gateway"] ? "border-destructive" : ""}`}
                  />
                  {validationErrors["bmc_config.gateway"] && (
                    <div className="flex items-center gap-2 text-sm text-destructive">
                      <AlertCircle className="h-4 w-4" />
                      <span>{validationErrors["bmc_config.gateway"]}</span>
                    </div>
                  )}
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

      {/* Power action confirmation dialog (off / cycle are destructive) */}
      {pendingAction && (
        <PowerConfirmDialog
          open={confirmDialogOpen}
          onOpenChange={(open) => {
            setConfirmDialogOpen(open);
            if (!open) setPendingAction(null);
          }}
          action={pendingAction}
          onConfirm={handleConfirmDestructive}
        />
      )}
    </>
  );
}
