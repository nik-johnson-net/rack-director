import type { RoleWithOs } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { Shield } from "lucide-react";

function diskLayoutSummary(role: RoleWithOs): string {
  const diskCount = role.disk_layout.disks.length;
  const partCount = role.disk_layout.disks.reduce(
    (sum, d) => sum + d.partitions.length,
    0
  );
  const hasLvm = role.disk_layout.volume_groups && role.disk_layout.volume_groups.length > 0;
  const parts = [`${diskCount} disk${diskCount !== 1 ? "s" : ""}`];
  if (hasLvm) {
    parts.push("LVM");
  } else if (partCount > 0) {
    parts.push(`${partCount} partition${partCount !== 1 ? "s" : ""}`);
  }
  return parts.join(", ");
}

function Roles() {
  const data = useLoaderData<RoleWithOs[]>();
  const navigate = useNavigate();

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Roles" },
        ]}
        title="Roles"
        description="Define how devices should be configured"
        actions={
          <Button onClick={() => navigate("/roles/new")}>
            + Create Role
          </Button>
        }
      />

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Name", "OS", "Firmware", "Disk Layout", "Devices", ""] as const).map(
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
                <td colSpan={6}>
                  <EmptyState
                    icon={Shield}
                    title="No roles defined"
                    description="Create a role to define how devices should be provisioned."
                    action={{
                      label: "+ Create Role",
                      onClick: () => navigate("/roles/new"),
                    }}
                  />
                </td>
              </tr>
            ) : (
              data.map((role, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={role.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {role.name}
                    </td>

                    {/* OS */}
                    <td className="px-3 py-2 text-xs text-text-primary">
                      {role.os_name} {role.os_version}
                    </td>

                    {/* Firmware */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {role.firmware_mode ?? "—"}
                    </td>

                    {/* Disk Layout */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {diskLayoutSummary(role)}
                    </td>

                    {/* Devices */}
                    <td className="px-3 py-2 text-xs text-text-muted">
                      —
                    </td>

                    {/* Edit link */}
                    <td className="px-3 py-2">
                      <button
                        onClick={() => navigate(`/roles/${role.id}`)}
                        className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                        aria-label={`Edit role ${role.name}`}
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

export default Roles;
