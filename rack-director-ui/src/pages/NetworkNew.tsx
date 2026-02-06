import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { FormField } from "@/components/ui/form-field";
import { createNetwork, ValidationError } from "@/lib/client";
import { useFieldErrors } from "@/hooks/useFieldErrors";

export default function NetworkNew() {
  const navigate = useNavigate();
  const { clearAllErrors, clearFieldError, setErrors, getError } = useFieldErrors();
  const [name, setName] = useState("");
  const [subnet, setSubnet] = useState("");
  const [gateway, setGateway] = useState("");
  const [dnsServers, setDnsServers] = useState("");
  const [leaseDuration, setLeaseDuration] = useState("");
  const [relayAgent, setRelayAgent] = useState("");
  const [enableAutodiscovery, setEnableAutodiscovery] = useState(false);
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
        enable_autodiscovery: enableAutodiscovery,
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

              {/* Row 2: Gateway + Lease Duration */}
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
                id="leaseDuration"
                label="Lease Duration (seconds)"
                type="number"
                required
                value={leaseDuration}
                onChange={setLeaseDuration}
                placeholder="e.g., 86400"
                error={getError("lease_duration")}
                onClearError={() => clearFieldError("lease_duration")}
              />

              {/* Row 3: DNS Servers (full width) */}
              <FormField
                id="dnsServers"
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

              {/* Row 4: Relay Agent (full width) */}
              <FormField
                id="relayAgent"
                label="Relay Agent Address"
                value={relayAgent}
                onChange={setRelayAgent}
                placeholder="Leave empty for Local L2"
                helperText="Leave empty if this DHCP server is on the same L2 network. Otherwise, specify the relay agent IP address."
                error={getError("relay_agent_address")}
                onClearError={() => clearFieldError("relay_agent_address")}
                className="sm:col-span-2"
              />

              {/* Row 5: Enable Autodiscovery (full width) */}
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
                <p className="text-sm text-muted-foreground">
                  When enabled, unknown devices will receive PXE boot options. When disabled, only known devices and pending devices will boot.
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
