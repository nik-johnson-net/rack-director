import { useState } from "react";
import type { OsmOperatingSystem, OsmModule } from "@/lib/client";
import { useLoaderData } from "react-router";
import { Link } from "react-router";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { HardDrive } from "lucide-react";
import { selectClassName } from "@/components/roles/styles";

type LoaderData = {
  operatingSystems: OsmOperatingSystem[];
  modules: OsmModule[];
};

function OperatingSystems() {
  const { operatingSystems, modules } = useLoaderData<LoaderData>();
  const [moduleFilter, setModuleFilter] = useState<string>("all");
  const [showDisabled, setShowDisabled] = useState(false);

  const moduleMap = new Map(modules.map((m) => [m.id, m]));

  const filtered = operatingSystems.filter((os) => {
    if (!showDisabled && os.disabled) return false;
    if (moduleFilter !== "all" && os.module_id !== parseInt(moduleFilter)) return false;
    return true;
  });

  const columns = ["Name", "Release", "Module", "Architectures", "Status"] as const;

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Images" },
        ]}
        title="Operating Systems"
        description="Operating system entries from installed OS modules"
      />

      {/* Filter Controls */}
      <div className="flex items-center gap-4 mb-3">
        <select
          value={moduleFilter}
          onChange={(e) => setModuleFilter(e.target.value)}
          className={`${selectClassName} w-auto min-w-[160px]`}
          aria-label="Filter by module"
        >
          <option value="all">All Modules</option>
          {modules.map((m) => (
            <option key={m.id} value={String(m.id)}>
              {m.name}
            </option>
          ))}
        </select>

        <label className="flex items-center gap-2 text-xs text-text-secondary cursor-pointer select-none">
          <input
            type="checkbox"
            checked={showDisabled}
            onChange={(e) => setShowDisabled(e.target.checked)}
            className="accent-accent cursor-pointer"
          />
          Show disabled
        </label>
      </div>

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {columns.map((col, i) => (
                <th
                  key={i}
                  className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                >
                  {col}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {filtered.length === 0 ? (
              <tr>
                <td colSpan={columns.length}>
                  <EmptyState
                    icon={HardDrive}
                    title="No operating systems found"
                    description="Upload an OS module to add operating system entries."
                    action={{
                      label: "Upload OS Module",
                      onClick: () => { window.location.href = "/osm/upload"; },
                    }}
                  />
                </td>
              </tr>
            ) : (
              filtered.map((os, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                const mod = moduleMap.get(os.module_id);
                const architectures = os.config.architectures
                  .map((a) => a.arch)
                  .join(", ") || "—";

                return (
                  <tr
                    key={os.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors ${os.disabled ? "opacity-60" : ""}`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      {os.name}
                    </td>

                    {/* Release */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {os.release || "—"}
                    </td>

                    {/* Module */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {mod ? (
                        <Link
                          to={`/osm/${os.module_id}`}
                          className="text-accent hover:text-accent-hover transition-colors"
                        >
                          {mod.name}
                        </Link>
                      ) : (
                        <span className="text-text-muted">—</span>
                      )}
                    </td>

                    {/* Architectures */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {architectures}
                    </td>

                    {/* Status */}
                    <td className="px-3 py-2 text-xs">
                      {os.disabled ? (
                        <span className="text-text-muted bg-bg-raised border border-border px-1.5 py-0.5 rounded-sm text-[10px] uppercase tracking-wider">
                          Disabled
                        </span>
                      ) : (
                        <span className="text-status-provisioned bg-status-provisioned-bg border border-status-provisioned px-1.5 py-0.5 rounded-sm text-[10px] uppercase tracking-wider">
                          Enabled
                        </span>
                      )}
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
