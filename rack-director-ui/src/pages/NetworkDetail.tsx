import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import PoolsTable from "@/components/networks/pools-table";
import ReservationsTable from "@/components/networks/reservations-table";
import LeasesTable from "@/components/networks/leases-table";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { FormFieldError } from "@/components/ui/form-field-error";
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

type Tab = "info" | "pools" | "reservations" | "leases";

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

  const [activeTab, setActiveTab] = useState<Tab>("info");

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

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: "info", label: "Network Info" },
    { id: "pools", label: "Pools", count: pools.length },
    { id: "reservations", label: "Static Reservations", count: reservations.length },
    { id: "leases", label: "Active Leases", count: leases.length },
  ];

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Networks", href: "/networks" },
          { label: network.name },
        ]}
        title={network.name}
        description="Configure network settings, pools, and reservations"
        actions={
          <Button variant="secondary" onClick={() => navigate("/networks")}>
            Back to Networks
          </Button>
        }
      />

      {/* Page tabs */}
      <div className="flex border-b border-border mb-6">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`px-4 py-2 text-xs transition-colors cursor-pointer ${
              activeTab === tab.id
                ? "text-text-primary border-b-2 border-accent -mb-px"
                : "text-text-secondary hover:text-text-primary"
            }`}
          >
            {tab.label}
            {tab.count !== undefined && (
              <span className="ml-2 px-1.5 py-0.5 bg-bg-raised text-text-muted text-xs rounded-sm">
                {tab.count}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Network Info Tab */}
      {activeTab === "info" && (
        <form onSubmit={handleSubmit}>
          {error && (
            <div className="mb-4 px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
              {error}
            </div>
          )}
          {successMessage && (
            <div className="mb-4 px-3 py-2 bg-status-provisioned-bg border-l-[3px] border-status-provisioned text-xs text-status-provisioned">
              {successMessage}
            </div>
          )}

          <div className="border border-border bg-bg-surface mb-4">
            <div className="px-3 py-2 border-b border-border">
              <span className="text-sm font-semibold text-text-primary">Network Configuration</span>
            </div>
            <div className="p-4 grid grid-cols-1 sm:grid-cols-2 gap-4">
              {/* Name */}
              <div>
                <label
                  htmlFor="name"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  Network Name <span className="text-status-broken">*</span>
                </label>
                <input
                  id="name"
                  type="text"
                  value={name}
                  onChange={(e) => { setName(e.target.value); clearFieldError("name"); }}
                  placeholder="e.g., Main Network"
                  required
                  aria-invalid={!!getError("name")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <FormFieldError error={getError("name")} />
              </div>

              {/* Subnet */}
              <div>
                <label
                  htmlFor="subnet"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  Subnet (CIDR) <span className="text-status-broken">*</span>
                </label>
                <input
                  id="subnet"
                  type="text"
                  value={subnet}
                  onChange={(e) => { setSubnet(e.target.value); clearFieldError("subnet"); }}
                  placeholder="e.g., 192.168.1.0/24"
                  required
                  aria-invalid={!!getError("subnet")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <FormFieldError error={getError("subnet")} />
              </div>

              {/* Gateway */}
              <div>
                <label
                  htmlFor="gateway"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  Gateway <span className="text-status-broken">*</span>
                </label>
                <input
                  id="gateway"
                  type="text"
                  value={gateway}
                  onChange={(e) => { setGateway(e.target.value); clearFieldError("gateway"); }}
                  placeholder="e.g., 192.168.1.1"
                  required
                  aria-invalid={!!getError("gateway")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <FormFieldError error={getError("gateway")} />
              </div>

              {/* Lease Duration */}
              <div>
                <label
                  htmlFor="leaseDuration"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  Lease Duration (seconds) <span className="text-status-broken">*</span>
                </label>
                <input
                  id="leaseDuration"
                  type="number"
                  value={leaseDuration}
                  onChange={(e) => { setLeaseDuration(e.target.value); clearFieldError("lease_duration"); }}
                  placeholder="e.g., 86400"
                  required
                  aria-invalid={!!getError("lease_duration")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <FormFieldError error={getError("lease_duration")} />
              </div>

              {/* DNS Servers */}
              <div className="sm:col-span-2">
                <label
                  htmlFor="dnsServers"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  DNS Servers <span className="text-status-broken">*</span>
                </label>
                <input
                  id="dnsServers"
                  type="text"
                  value={dnsServers}
                  onChange={(e) => { setDnsServers(e.target.value); clearFieldError("dns_servers"); }}
                  placeholder="e.g., 8.8.8.8, 8.8.4.4"
                  required
                  aria-invalid={!!getError("dns_servers")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <p className="text-xs text-text-muted mt-1">Enter multiple DNS servers separated by commas</p>
                <FormFieldError error={getError("dns_servers")} />
              </div>

              {/* Relay Agent */}
              <div className="sm:col-span-2">
                <label
                  htmlFor="relayAgent"
                  className="block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1"
                >
                  Relay Agent Address
                </label>
                <input
                  id="relayAgent"
                  type="text"
                  value={relayAgent}
                  onChange={(e) => { setRelayAgent(e.target.value); clearFieldError("relay_agent_address"); }}
                  placeholder="Leave empty for Local L2"
                  aria-invalid={!!getError("relay_agent_address")}
                  className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted"
                />
                <p className="text-xs text-text-muted mt-1">
                  Leave empty if this DHCP server is on the same L2 network. Otherwise, specify the relay agent IP address.
                </p>
                <FormFieldError error={getError("relay_agent_address")} />
              </div>

              {/* Autodiscovery toggle */}
              <div className="sm:col-span-2">
                <label className="flex items-center gap-2 cursor-pointer select-none">
                  <input
                    type="checkbox"
                    id="enableAutodiscovery"
                    checked={enableAutodiscovery}
                    onChange={(e) => setEnableAutodiscovery(e.target.checked)}
                    className="w-3.5 h-3.5 accent-accent cursor-pointer"
                  />
                  <span className="text-xs font-semibold text-text-secondary uppercase tracking-[0.5px]">
                    Enable Autodiscovery
                  </span>
                </label>
                <p className="text-xs text-text-muted mt-1 ml-5">
                  When enabled, unknown devices will receive PXE boot options. When disabled, only known devices and pending devices will boot.
                </p>
              </div>
            </div>
          </div>

          <div className="flex gap-2">
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? "Saving..." : "Save Changes"}
            </Button>
            <Button type="button" variant="secondary" onClick={() => navigate("/networks")}>
              Cancel
            </Button>
          </div>
        </form>
      )}

      {/* Pools Tab */}
      {activeTab === "pools" && (
        <div className="border border-border bg-bg-surface">
          <div className="px-3 py-2 border-b border-border flex items-center justify-between">
            <span className="text-sm font-semibold text-text-primary">IP Address Pools</span>
          </div>
          <div className="p-4">
            <PoolsTable networkId={networkId} pools={pools} onPoolsChange={setPools} />
          </div>
        </div>
      )}

      {/* Reservations Tab */}
      {activeTab === "reservations" && (
        <div className="border border-border bg-bg-surface">
          <div className="px-3 py-2 border-b border-border">
            <span className="text-sm font-semibold text-text-primary">Static Reservations</span>
          </div>
          <div className="p-4">
            <ReservationsTable
              networkId={networkId}
              reservations={reservations}
              onReservationsChange={setReservations}
            />
          </div>
        </div>
      )}

      {/* Leases Tab */}
      {activeTab === "leases" && (
        <div className="border border-border bg-bg-surface">
          <div className="px-3 py-2 border-b border-border">
            <span className="text-sm font-semibold text-text-primary">Active Leases</span>
          </div>
          <div className="p-4">
            <LeasesTable
              network={network}
              networkId={networkId}
              leases={leases}
              pendingDevices={pendingDevices}
              onReservationCreated={(reservation) => {
                setReservations([...reservations, reservation]);
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

export default NetworkDetail;
