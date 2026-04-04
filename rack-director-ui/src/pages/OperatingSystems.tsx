import type { OperatingSystem } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { HardDrive } from "lucide-react";

function OperatingSystems() {
  const data = useLoaderData<OperatingSystem[]>();
  const navigate = useNavigate();

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Images" },
        ]}
        title="Operating Systems"
        description="OS images and install scripts for provisioning"
        actions={
          <Button onClick={() => navigate("/operating-systems/new")}>
            + Add OS
          </Button>
        }
      />

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Name", "Version", "Architectures", "Used By", ""] as const).map(
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
                <td colSpan={5}>
                  <EmptyState
                    icon={HardDrive}
                    title="No operating systems defined"
                    description="Add an OS image to use when provisioning devices."
                    action={{
                      label: "+ Add OS",
                      onClick: () => navigate("/operating-systems/new"),
                    }}
                  />
                </td>
              </tr>
            ) : (
              data.map((os, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={os.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {os.name}
                    </td>

                    {/* Version */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {os.version}
                    </td>

                    {/* Architectures — placeholder, OperatingSystem list type has no arch info */}
                    <td className="px-3 py-2 text-xs text-text-muted">
                      —
                    </td>

                    {/* Used By */}
                    <td className="px-3 py-2 text-xs text-text-muted">
                      —
                    </td>

                    {/* Edit link */}
                    <td className="px-3 py-2">
                      <button
                        onClick={() => navigate(`/operating-systems/${os.id}`)}
                        className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        aria-label={`Edit ${os.name} ${os.version}`}
                      >
                        edit
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

export default OperatingSystems;
