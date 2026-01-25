import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { createNetwork, ValidationError } from "@/lib/client";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { FormFieldError } from "@/components/ui/form-field-error";

export default function NetworkNew() {
  const navigate = useNavigate();
  const { clearAllErrors, clearFieldError, setErrors, hasError, getError } = useFieldErrors();
  const [name, setName] = useState("");
  const [subnet, setSubnet] = useState("");
  const [gateway, setGateway] = useState("");
  const [dnsServers, setDnsServers] = useState("");
  const [leaseDuration, setLeaseDuration] = useState("");
  const [relayAgent, setRelayAgent] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    clearAllErrors();
    setIsSubmitting(true);

    try {
      // Parse DNS servers
      const dnsArray = dnsServers
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0);

      // Create network
      const network = await createNetwork({
        name,
        subnet,
        gateway,
        dns_servers: dnsArray,
        lease_duration: parseInt(leaseDuration),
        relay_agent_address: relayAgent || undefined,
      });

      navigate(`/networks/${network.id}`);
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
        setError("Please fix the validation errors below");
      } else {
        setError(err instanceof Error ? err.message : "Failed to create network");
      }
      setIsSubmitting(false);
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Networks", href: "/networks" },
          { label: "New Network" },
        ]}
        title="Add DHCP Network"
        description="Create a new DHCP network to manage IP address allocation"
      />

      <Card>
        <CardHeader>
          <CardTitle>Network Configuration</CardTitle>
          <CardDescription>
            Configure the network subnet, gateway, and DNS settings
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            {error && (
              <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
                {error}
              </div>
            )}

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              {/* Row 1: Name + Subnet */}
              <div className="space-y-2">
                <Label htmlFor="name">Network Name</Label>
                <Input
                  id="name"
                  type="text"
                  placeholder="e.g., Main Network"
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    clearFieldError("name");
                  }}
                  aria-invalid={hasError("name")}
                  required
                />
                <FormFieldError error={getError("name")} />
              </div>

              <div className="space-y-2">
                <Label htmlFor="subnet">Subnet (CIDR)</Label>
                <Input
                  id="subnet"
                  type="text"
                  placeholder="e.g., 192.168.1.0/24"
                  value={subnet}
                  onChange={(e) => {
                    setSubnet(e.target.value);
                    clearFieldError("subnet");
                  }}
                  aria-invalid={hasError("subnet")}
                  required
                />
                <FormFieldError error={getError("subnet")} />
              </div>

              {/* Row 2: Gateway + Lease Duration */}
              <div className="space-y-2">
                <Label htmlFor="gateway">Gateway</Label>
                <Input
                  id="gateway"
                  type="text"
                  placeholder="e.g., 192.168.1.1"
                  value={gateway}
                  onChange={(e) => {
                    setGateway(e.target.value);
                    clearFieldError("gateway");
                  }}
                  aria-invalid={hasError("gateway")}
                  required
                />
                <FormFieldError error={getError("gateway")} />
              </div>

              <div className="space-y-2">
                <Label htmlFor="leaseDuration">Lease Duration (seconds)</Label>
                <Input
                  id="leaseDuration"
                  type="number"
                  placeholder="e.g., 86400"
                  value={leaseDuration}
                  onChange={(e) => {
                    setLeaseDuration(e.target.value);
                    clearFieldError("lease_duration");
                  }}
                  aria-invalid={hasError("lease_duration")}
                  required
                />
                <FormFieldError error={getError("lease_duration")} />
              </div>

              {/* Row 3: DNS Servers (full width) */}
              <div className="space-y-2 sm:col-span-2">
                <Label htmlFor="dnsServers">DNS Servers</Label>
                <Input
                  id="dnsServers"
                  type="text"
                  placeholder="e.g., 8.8.8.8, 8.8.4.4"
                  value={dnsServers}
                  onChange={(e) => {
                    setDnsServers(e.target.value);
                    clearFieldError("dns_servers");
                  }}
                  aria-invalid={hasError("dns_servers")}
                  required
                />
                <FormFieldError error={getError("dns_servers")} />
                <p className="text-sm text-muted-foreground">
                  Enter multiple DNS servers separated by commas
                </p>
              </div>

              {/* Row 4: Relay Agent (full width) */}
              <div className="space-y-2 sm:col-span-2">
                <Label htmlFor="relayAgent">Relay Agent Address</Label>
                <Input
                  id="relayAgent"
                  type="text"
                  placeholder="Leave empty for Local L2"
                  value={relayAgent}
                  onChange={(e) => {
                    setRelayAgent(e.target.value);
                    clearFieldError("relay_agent_address");
                  }}
                  aria-invalid={hasError("relay_agent_address")}
                />
                <FormFieldError error={getError("relay_agent_address")} />
                <p className="text-sm text-muted-foreground">
                  Leave empty if this DHCP server is on the same L2 network. Otherwise, specify the relay agent IP address.
                </p>
              </div>
            </div>

            <div className="flex gap-2">
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Creating..." : "Create Network"}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => navigate("/networks")}
                disabled={isSubmitting}
              >
                Cancel
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
