import { useState } from "react";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import {
  deleteOsmModule,
  disableOsmOs,
  enableOsmOs,
  getOsmModuleExportUrl,
  type OsmModule,
  type OsmOperatingSystem,
} from "@/lib/client";
import { Download, Trash2 } from "lucide-react";

interface LoaderData {
  module: OsmModule;
  operatingSystems: OsmOperatingSystem[];
}

// ── Helper components ─────────────────────────────────────────────────────────

function SectionCard({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border bg-bg-surface">
      <div className="px-4 py-3 border-b border-border">
        <span className="text-sm font-semibold text-text-primary">{title}</span>
      </div>
      <div className="px-4 py-4">{children}</div>
    </div>
  );
}

function KVRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="grid gap-x-4 py-1" style={{ gridTemplateColumns: "140px 1fr" }}>
      <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">
        {label}
      </span>
      <span className="text-xs text-text-primary">{value}</span>
    </div>
  );
}

function sourceBadge(source: string) {
  if (source === "uploaded") {
    return (
      <span className="inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.5px] bg-accent/15 text-accent rounded-[2px]">
        uploaded
      </span>
    );
  }
  return (
    <span className="inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.5px] bg-bg-raised text-text-muted rounded-[2px]">
      bundled
    </span>
  );
}

function formatDate(dateStr?: string): string {
  if (!dateStr) return "—";
  try {
    return new Date(dateStr).toLocaleString();
  } catch {
    return dateStr;
  }
}

// ── Page component ────────────────────────────────────────────────────────────

function OsmModuleDetail() {
  const loaderData = useLoaderData<LoaderData>();
  const navigate = useNavigate();

  const osmModule = loaderData.module;
  const [operatingSystems, setOperatingSystems] = useState<OsmOperatingSystem[]>(
    loaderData.operatingSystems
  );
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [togglingOsId, setTogglingOsId] = useState<number | null>(null);
  const [toggleError, setToggleError] = useState<string | null>(null);

  const handleDelete = async () => {
    setDeleteError(null);
    try {
      await deleteOsmModule(osmModule.id);
      navigate("/osm");
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to delete module";
      setDeleteError(msg);
      throw err; // re-throw so dialog stays open
    }
  };

  const handleToggleOs = async (os: OsmOperatingSystem) => {
    setTogglingOsId(os.id);
    setToggleError(null);
    try {
      if (os.disabled) {
        await enableOsmOs(os.id);
      } else {
        await disableOsmOs(os.id);
      }
      setOperatingSystems((prev) =>
        prev.map((o) => (o.id === os.id ? { ...o, disabled: !o.disabled } : o))
      );
    } catch (err) {
      setToggleError(
        err instanceof Error ? err.message : "Failed to update OS status"
      );
    } finally {
      setTogglingOsId(null);
    }
  };

  const canExport = osmModule.source === "uploaded" && !!osmModule.archive_path;

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Modules", href: "/osm" },
          { label: osmModule.name },
        ]}
        title={osmModule.name}
        description={osmModule.description || "OS module details"}
        actions={
          <div className="flex gap-2">
            {canExport && (
              <a
                href={getOsmModuleExportUrl(osmModule.id)}
                download
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium border border-border bg-bg-surface text-text-primary hover:bg-bg-raised transition-colors rounded-[4px]"
                aria-label={`Export module ${osmModule.name}`}
              >
                <Download className="h-3.5 w-3.5" />
                Export
              </a>
            )}
            {!osmModule.is_default && (
              <Button variant="danger" onClick={() => setDeleteDialogOpen(true)}>
                <Trash2 className="h-3.5 w-3.5" />
                Delete
              </Button>
            )}
          </div>
        }
      />

      {deleteError && (
        <div className="mb-4 px-3 py-2 border border-status-broken/40 bg-status-broken-bg text-status-broken text-xs">
          {deleteError}
        </div>
      )}

      {toggleError && (
        <div className="mb-4 px-3 py-2 border border-status-broken/40 bg-status-broken-bg text-status-broken text-xs">
          {toggleError}
        </div>
      )}

      <div className="space-y-4" style={{ maxWidth: 900 }}>
        {/* Module info KV */}
        <SectionCard title="Module Info">
          <div className="space-y-0.5">
            <KVRow label="Version" value={osmModule.version || <span className="text-text-muted">—</span>} />
            <KVRow label="Author" value={osmModule.author || <span className="text-text-muted">—</span>} />
            <KVRow label="Source" value={sourceBadge(osmModule.source)} />
            <KVRow
              label="Description"
              value={
                osmModule.description ? (
                  osmModule.description
                ) : (
                  <span className="text-text-muted">—</span>
                )
              }
            />
            <KVRow label="Created" value={formatDate(osmModule.created_at)} />
            <KVRow label="Operating Systems" value={operatingSystems.length} />
          </div>
        </SectionCard>

        {/* Operating systems table */}
        <SectionCard title={`Operating Systems (${operatingSystems.length})`}>
          {operatingSystems.length === 0 ? (
            <p className="text-xs text-text-muted">No operating systems in this module.</p>
          ) : (
            <table className="w-full border-collapse">
              <thead>
                <tr className="bg-bg-raised">
                  {["Name", "Release", "Architectures", "Status", ""].map((col, i) => (
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
                {operatingSystems.map((os, idx) => {
                  const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                  const isToggling = togglingOsId === os.id;
                  const archs =
                    os.config.architectures.map((a) => a.arch).join(", ") || "—";

                  return (
                    <tr
                      key={os.id}
                      className={`${rowBg} border-b border-border-muted last:border-b-0 ${os.disabled ? "opacity-60" : ""}`}
                    >
                      <td className="px-3 py-2 text-xs text-text-primary font-medium">
                        {os.name}
                      </td>
                      <td className="px-3 py-2 text-xs text-text-secondary">
                        {os.release || "—"}
                      </td>
                      <td className="px-3 py-2 text-xs text-text-secondary">
                        {archs}
                      </td>
                      <td className="px-3 py-2">
                        {os.disabled ? (
                          <span className="inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.5px] bg-bg-raised text-text-muted rounded-[2px]">
                            disabled
                          </span>
                        ) : (
                          <span className="inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.5px] bg-status-provisioned-bg text-status-provisioned rounded-[2px]">
                            enabled
                          </span>
                        )}
                      </td>
                      <td className="px-3 py-2">
                        <button
                          onClick={() => handleToggleOs(os)}
                          disabled={isToggling}
                          aria-label={os.disabled ? `Enable ${os.name}` : `Disable ${os.name}`}
                          className={`text-xs transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed ${
                            os.disabled
                              ? "text-accent hover:text-accent-hover"
                              : "text-text-muted hover:text-text-secondary"
                          }`}
                        >
                          {isToggling
                            ? "..."
                            : os.disabled
                            ? "Enable"
                            : "Disable"}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </SectionCard>
      </div>

      {!osmModule.is_default && (
        <DeleteConfirmationDialog
          open={deleteDialogOpen}
          onOpenChange={(open) => {
            setDeleteDialogOpen(open);
            if (!open) setDeleteError(null);
          }}
          title="Delete Module?"
          description="This will permanently delete this module and all its operating system definitions."
          itemName={osmModule.name}
          warningMessage={deleteError ?? undefined}
          onConfirm={handleDelete}
        />
      )}
    </div>
  );
}

export default OsmModuleDetail;
