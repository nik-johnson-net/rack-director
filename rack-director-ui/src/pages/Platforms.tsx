import type { Platform } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { Cpu } from "lucide-react";

function diskSummary(platform: Platform): string {
  const disks = platform.attributes.disks;
  if (disks.length === 0) return "—";
  const types = disks.map((d) => d.disk_type.toUpperCase());
  const unique = [...new Set(types)];
  return `${disks.length} disk${disks.length !== 1 ? "s" : ""} (${unique.join(", ")})`;
}

function nicSummary(platform: Platform): string {
  const nics = platform.attributes.nics;
  if (nics.length === 0) return "—";
  return `${nics.length} NIC${nics.length !== 1 ? "s" : ""}`;
}

function Platforms() {
  const data = useLoaderData<Platform[]>();
  const navigate = useNavigate();

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Platforms" },
        ]}
        title="Platforms"
        description="Hardware configurations for device matching"
        actions={
          <Button onClick={() => navigate("/platforms/new")}>
            + Create Platform
          </Button>
        }
      />

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Name", "Firmware", "Disks", "NICs", "Memory", "Devices", ""] as const).map(
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
            {data.length === 0 ? (
              <tr>
                <td colSpan={7}>
                  <EmptyState
                    icon={Cpu}
                    title="No platforms defined"
                    description="Create a platform to group similar hardware configurations."
                    action={{
                      label: "+ Create Platform",
                      onClick: () => navigate("/platforms/new"),
                    }}
                  />
                </td>
              </tr>
            ) : (
              data.map((platform, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={platform.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {platform.name}
                    </td>

                    {/* Firmware */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      —
                    </td>

                    {/* Disks */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {diskSummary(platform)}
                    </td>

                    {/* NICs */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {nicSummary(platform)}
                    </td>

                    {/* Memory */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {platform.attributes.memory_gib > 0
                        ? `${platform.attributes.memory_gib} GiB`
                        : "—"}
                    </td>

                    {/* Device count */}
                    <td className="px-3 py-2 text-xs text-text-muted">
                      —
                    </td>

                    {/* View link */}
                    <td className="px-3 py-2">
                      <button
                        onClick={() => navigate(`/platforms/${platform.id}`)}
                        className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        aria-label={`View platform ${platform.name}`}
                      >
                        view
                      </button>
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

export default Platforms;
