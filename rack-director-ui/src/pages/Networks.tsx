import { useState, useEffect } from "react";
import type { DhcpNetwork } from "@/lib/client";
import { getPoolsForNetwork, deleteNetwork } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { Network } from "lucide-react";
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

type NetworkWithLeaseCounts = DhcpNetwork & { poolCount?: number };

function AutodiscoveryBadge({ enabled }: { enabled: boolean }) {
  if (enabled) {
    return (
      <span className="inline-flex items-center gap-1.5 px-2 py-0.5 text-xs font-medium rounded-sm bg-status-provisioned-bg text-status-provisioned">
        <span className="size-1.5 rounded-full bg-status-provisioned" />
        enabled
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 text-xs font-medium rounded-sm bg-status-removed-bg text-status-removed">
      <span className="size-1.5 rounded-full bg-status-removed" />
      disabled
    </span>
  );
}

function Networks() {
  const data = useLoaderData<DhcpNetwork[]>();
  const navigate = useNavigate();
  const [networks, setNetworks] = useState<NetworkWithLeaseCounts[]>(data);
  const [isDeleting, setIsDeleting] = useState(false);

  useEffect(() => {
    const fetchPoolCounts = async () => {
      const withCounts = await Promise.all(
        data.map(async (network) => {
          try {
            const pools = await getPoolsForNetwork(network.id);
            return { ...network, poolCount: pools.length };
          } catch {
            return { ...network, poolCount: 0 };
          }
        })
      );
      setNetworks(withCounts);
    };
    fetchPoolCounts();
  }, [data]);

  const handleDelete = async (id: number) => {
    setIsDeleting(true);
    try {
      await deleteNetwork(id);
      setNetworks((prev) => prev.filter((n) => n.id !== id));
    } catch (err) {
      console.error("Failed to delete network:", err);
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Networks" },
        ]}
        title="Networks"
        description="DHCP network configuration"
        actions={
          <Button onClick={() => navigate("/networks/new")}>
            + Create Network
          </Button>
        }
      />

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Name", "Subnet", "Gateway", "DNS", "Autodiscovery", "Pools", ""] as const).map(
                (col, i) => (
                  <th
                    key={i}
                    className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                  >
                    {col}
                  </th>
                )
              )}
            </tr>
          </thead>
          <tbody>
            {networks.length === 0 ? (
              <tr>
                <td colSpan={7}>
                  <EmptyState
                    icon={Network}
                    title="No networks defined"
                    description="Create a DHCP network to manage IP address allocation for your devices."
                    action={{
                      label: "+ Create Network",
                      onClick: () => navigate("/networks/new"),
                    }}
                  />
                </td>
              </tr>
            ) : (
              networks.map((network, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={network.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {network.name}
                    </td>

                    {/* Subnet */}
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {network.subnet}
                    </td>

                    {/* Gateway */}
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {network.gateway}
                    </td>

                    {/* DNS */}
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {network.dns_servers.length > 0
                        ? network.dns_servers.join(", ")
                        : <span className="text-text-muted">—</span>}
                    </td>

                    {/* Autodiscovery */}
                    <td className="px-3 py-2">
                      <AutodiscoveryBadge enabled={network.enable_autodiscovery} />
                    </td>

                    {/* Pool count */}
                    <td className="px-3 py-2 text-xs text-text-muted">
                      {network.poolCount !== undefined ? network.poolCount : (
                        <span className="text-text-muted italic">…</span>
                      )}
                    </td>

                    {/* Actions */}
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-3">
                        <button
                          onClick={() => navigate(`/networks/${network.id}`)}
                          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                          aria-label={`View network ${network.name}`}
                        >
                          view
                        </button>
                        <AlertDialog>
                          <AlertDialogTrigger asChild>
                            <button
                              className="text-xs text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                              aria-label={`Delete network ${network.name}`}
                            >
                              delete
                            </button>
                          </AlertDialogTrigger>
                          <AlertDialogContent>
                            <AlertDialogHeader>
                              <AlertDialogTitle>Delete Network</AlertDialogTitle>
                              <AlertDialogDescription>
                                Are you sure you want to delete "{network.name}"? This will also delete all
                                associated pools and static reservations. This action cannot be undone.
                              </AlertDialogDescription>
                            </AlertDialogHeader>
                            <AlertDialogFooter>
                              <AlertDialogCancel>Cancel</AlertDialogCancel>
                              <AlertDialogAction
                                onClick={() => handleDelete(network.id)}
                                disabled={isDeleting}
                              >
                                {isDeleting ? "Deleting..." : "Delete"}
                              </AlertDialogAction>
                            </AlertDialogFooter>
                          </AlertDialogContent>
                        </AlertDialog>
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default Networks;
