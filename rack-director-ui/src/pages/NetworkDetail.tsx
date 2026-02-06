import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { FormField } from "@/components/ui/form-field";
import PoolsTable from "@/components/networks/pools-table";
import ReservationsTable from "@/components/networks/reservations-table";
import LeasesTable from "@/components/networks/leases-table";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import {
  updateNetwork,
  ValidationError,
  type DhcpNetwork,
  type DhcpPool,
  type StaticReservation,
  type DhcpLease,
  type PendingDevice,
} from "@/lib/client";

type LoaderData = {
  network: DhcpNetwork;
  pools: DhcpPool[];
  reservations: StaticReservation[];
  leases: DhcpLease[];
  pendingDevices: PendingDevice[];
};

function NetworkDetail() {
  const initialData = useLoaderData<LoaderData>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const networkId = parseInt(params.id!);
  const { clearAllErrors, clearFieldError, setErrors, getError } = useFieldErrors();

  const [network, setNetwork] = useState(initialData.network);
  const [pools, setPools] = useState(initialData.pools);
  const [reservations, setReservations] = useState(initialData.reservations);
  const [leases] = useState(initialData.leases);
  const [pendingDevices] = useState(initialData.pendingDevices);

  const [name, setName] = useState(network.name);
  const [subnet, setSubnet] = useState(network.subnet);
  const [gateway, setGateway] = useState(network.gateway);
  const [dnsServers, setDnsServers] = useState(network.dns_servers.join(", "));
  const [leaseDuration, setLeaseDuration] = useState(network.lease_duration.toString());
  const [relayAgent, setRelayAgent] = useState(network.relay_agent_address || "");
  const [enableAutodiscovery, setEnableAutodiscovery] = useState(network.enable_autodiscovery);

  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  const isDefaultNetwork = networkId === 1;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSuccessMessage(null);
    clearAllErrors();
    setIsSubmitting(true);

    try {
      const dnsArray = dnsServers
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0);

      const updated = await updateNetwork(networkId, {
        name,
        subnet,
        gateway,
        dns_servers: dnsArray,
        lease_duration: parseInt(leaseDuration),
        // Explicitly send null when empty to clear the relay agent
        relay_agent_address: relayAgent,
        enable_autodiscovery: enableAutodiscovery,
      });

      setNetwork(updated);
      setSuccessMessage("Network updated successfully");
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
        setError("Please fix the validation errors below");
      } else {
        setError(err instanceof Error ? err.message : "Failed to update network");
      }
    } finally {
      setIsSubmitting(false);
    }
  };

  useEffect(() => {
    if (successMessage) {
      const timer = setTimeout(() => setSuccessMessage(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [successMessage]);

  return (
    <div className="space-y-6 max-w-5xl">
      <PageHeader
        breadcrumbs={[{ label: "Networks", href: "/networks" }, { label: network.name }]}
        title={network.name}
        description="Configure network settings, pools, and reservations"
        status={
          isDefaultNetwork ? (
            <Badge variant="outline">Default</Badge>
          ) : undefined
        }
        actions={
          <Button variant="outline" onClick={() => navigate("/networks")}>
            Back to Networks
          </Button>
        }
      />

      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
          {error}
        </div>
      )}

      {successMessage && (
        <div className="bg-green-50 border border-green-200 text-green-800 px-4 py-3 rounded-md">
          {successMessage}
        </div>
      )}

      <Tabs defaultValue="info" className="space-y-4">
        <TabsList>
          <TabsTrigger value="info">Network Info</TabsTrigger>
          <TabsTrigger value="pools">
            Pools
            <Badge variant="secondary" className="ml-2">
              {pools.length}
            </Badge>
          </TabsTrigger>
          <TabsTrigger value="reservations">
            Static Reservations
            <Badge variant="secondary" className="ml-2">
              {reservations.length}
            </Badge>
          </TabsTrigger>
          <TabsTrigger value="leases">
            Active Leases
            <Badge variant="secondary" className="ml-2">
              {leases.length}
            </Badge>
          </TabsTrigger>
        </TabsList>

        <TabsContent value="info" className="space-y-4">
          <form onSubmit={handleSubmit} className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>Network Configuration</CardTitle>
                <CardDescription>
                  Configure the network subnet, gateway, and DNS settings
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <FormField
                    id="name"
                    label="Network Name"
                    required
                    value={name}
                    onChange={setName}
                    placeholder="e.g., Main Network"
                    error={getError("name")}
                    onClearError={() => clearFieldError("name")}
                  />

                  <FormField
                    id="subnet"
                    label="Subnet (CIDR)"
                    required
                    value={subnet}
                    onChange={setSubnet}
                    placeholder="e.g., 192.168.1.0/24"
                    error={getError("subnet")}
                    onClearError={() => clearFieldError("subnet")}
                  />

                  <FormField
                    id="gateway"
                    label="Gateway"
                    required
                    value={gateway}
                    onChange={setGateway}
                    placeholder="e.g., 192.168.1.1"
                    error={getError("gateway")}
                    onClearError={() => clearFieldError("gateway")}
                  />

                  <FormField
                    id="lease-duration"
                    label="Lease Duration (seconds)"
                    type="number"
                    required
                    value={leaseDuration}
                    onChange={setLeaseDuration}
                    placeholder="e.g., 86400"
                    error={getError("lease_duration")}
                    onClearError={() => clearFieldError("lease_duration")}
                  />

                  <FormField
                    id="dns-servers"
                    label="DNS Servers"
                    required
                    value={dnsServers}
                    onChange={setDnsServers}
                    placeholder="e.g., 8.8.8.8, 8.8.4.4"
                    helperText="Enter multiple DNS servers separated by commas"
                    error={getError("dns_servers")}
                    onClearError={() => clearFieldError("dns_servers")}
                    className="sm:col-span-2"
                  />

                  <FormField
                    id="relay-agent"
                    label="Relay Agent Address"
                    value={relayAgent}
                    onChange={setRelayAgent}
                    placeholder="Leave empty for Local L2"
                    helperText="Leave empty if this DHCP server is on the same L2 network. Otherwise, specify the relay agent IP address."
                    error={getError("relay_agent_address")}
                    onClearError={() => clearFieldError("relay_agent_address")}
                    className="sm:col-span-2"
                  />

                  <div className="space-y-2 sm:col-span-2">
                    <div className="flex items-center space-x-2">
                      <Checkbox
                        id="enableAutodiscovery"
                        checked={enableAutodiscovery}
                        onCheckedChange={(checked) => setEnableAutodiscovery(checked === true)}
                      />
                      <Label htmlFor="enableAutodiscovery" className="cursor-pointer">
                        Enable Autodiscovery
                      </Label>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      When enabled, unknown devices will receive PXE boot options. When disabled, only known devices and pending devices will boot.
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>

            <div className="flex justify-end gap-2">
              <Button type="button" variant="outline" onClick={() => navigate("/networks")}>
                Cancel
              </Button>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Saving..." : "Save Changes"}
              </Button>
            </div>
          </form>
        </TabsContent>

        <TabsContent value="pools">
          <Card>
            <CardHeader>
              <CardTitle>IP Address Pools</CardTitle>
              <CardDescription>
                Define ranges of IP addresses for dynamic allocation
              </CardDescription>
            </CardHeader>
            <CardContent>
              <PoolsTable networkId={networkId} pools={pools} onPoolsChange={setPools} />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="reservations">
          <Card>
            <CardHeader>
              <CardTitle>Static Reservations</CardTitle>
              <CardDescription>
                Assign specific IP addresses to MAC addresses
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ReservationsTable
                networkId={networkId}
                reservations={reservations}
                onReservationsChange={setReservations}
              />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="leases">
          <Card>
            <CardHeader>
              <CardTitle>Active Leases</CardTitle>
              <CardDescription>Currently assigned IP addresses from this network</CardDescription>
            </CardHeader>
            <CardContent>
              <LeasesTable
                network={network}
                networkId={networkId}
                leases={leases}
                pendingDevices={pendingDevices}
                onReservationCreated={(reservation) => {
                  setReservations([...reservations, reservation]);
                }}
              />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}

export default NetworkDetail;
