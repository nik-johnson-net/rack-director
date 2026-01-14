import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import PoolsTable from "@/components/networks/pools-table";
import ReservationsTable from "@/components/networks/reservations-table";
import LeasesTable from "@/components/networks/leases-table";
import {
  updateNetwork,
  type DhcpNetwork,
  type DhcpPool,
  type StaticReservation,
  type DhcpLease,
} from "@/lib/client";

type LoaderData = {
  network: DhcpNetwork;
  pools: DhcpPool[];
  reservations: StaticReservation[];
  leases: DhcpLease[];
};

function NetworkDetail() {
  const initialData = useLoaderData<LoaderData>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const networkId = parseInt(params.id!);

  const [network, setNetwork] = useState(initialData.network);
  const [pools, setPools] = useState(initialData.pools);
  const [reservations, setReservations] = useState(initialData.reservations);
  const [leases] = useState(initialData.leases);

  const [name, setName] = useState(network.name);
  const [subnet, setSubnet] = useState(network.subnet);
  const [gateway, setGateway] = useState(network.gateway);
  const [dnsServers, setDnsServers] = useState(network.dns_servers.join(", "));
  const [leaseDuration, setLeaseDuration] = useState(network.lease_duration.toString());
  const [relayAgent, setRelayAgent] = useState(network.relay_agent_address || "");

  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  const isDefaultNetwork = networkId === 1;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSuccessMessage(null);
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
        relay_agent_address: relayAgent || undefined,
      });

      setNetwork(updated);
      setSuccessMessage("Network updated successfully");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update network");
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
                  <div className="space-y-2">
                    <Label htmlFor="name">Network Name *</Label>
                    <Input
                      id="name"
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      placeholder="e.g., Main Network"
                      required
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="subnet">Subnet (CIDR) *</Label>
                    <Input
                      id="subnet"
                      value={subnet}
                      onChange={(e) => setSubnet(e.target.value)}
                      placeholder="e.g., 192.168.1.0/24"
                      required
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="gateway">Gateway *</Label>
                    <Input
                      id="gateway"
                      value={gateway}
                      onChange={(e) => setGateway(e.target.value)}
                      placeholder="e.g., 192.168.1.1"
                      required
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="lease-duration">Lease Duration (seconds) *</Label>
                    <Input
                      id="lease-duration"
                      type="number"
                      value={leaseDuration}
                      onChange={(e) => setLeaseDuration(e.target.value)}
                      placeholder="e.g., 86400"
                      required
                    />
                  </div>
                  <div className="space-y-2 sm:col-span-2">
                    <Label htmlFor="dns-servers">DNS Servers *</Label>
                    <Input
                      id="dns-servers"
                      value={dnsServers}
                      onChange={(e) => setDnsServers(e.target.value)}
                      placeholder="e.g., 8.8.8.8, 8.8.4.4"
                      required
                    />
                    <p className="text-xs text-muted-foreground">
                      Enter multiple DNS servers separated by commas
                    </p>
                  </div>
                  <div className="space-y-2 sm:col-span-2">
                    <Label htmlFor="relay-agent">Relay Agent Address (Optional)</Label>
                    <Input
                      id="relay-agent"
                      value={relayAgent}
                      onChange={(e) => setRelayAgent(e.target.value)}
                      placeholder="Leave empty for Local L2"
                    />
                    <p className="text-xs text-muted-foreground">
                      Leave empty if this DHCP server is on the same L2 network. Otherwise, specify
                      the relay agent IP address.
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
              <LeasesTable networkId={networkId} leases={leases} />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}

export default NetworkDetail;
