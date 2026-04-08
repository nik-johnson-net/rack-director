import { useState } from "react";
import type { OsmModule, OsmUpload } from "@/lib/client";
import { deleteOsmModule, getOsmModuleExportUrl } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { EmptyState } from "@/components/ui/empty-state";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { Package } from "lucide-react";

type LoaderData = {
  modules: OsmModule[];
  uploads: OsmUpload[];
};

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

function uploadStatusBadge(status: OsmUpload["status"]) {
  const map: Record<OsmUpload["status"], string> = {
    uploading: "text-accent bg-accent/15",
    validating: "text-status-new bg-status-new-bg",
    extracting: "text-status-unprovisioned bg-status-unprovisioned-bg",
    complete: "text-status-provisioned bg-status-provisioned-bg",
    failed: "text-status-broken bg-status-broken-bg",
  };
  const cls = map[status] ?? "text-text-muted bg-bg-raised";
  return (
    <span className={`inline-flex items-center px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.5px] rounded-[2px] ${cls}`}>
      {status}
    </span>
  );
}

function isRecentUpload(upload: OsmUpload): boolean {
  if (upload.status !== "complete" && upload.status !== "failed") return true;
  if (!upload.created_at) return false;
  const age = Date.now() - new Date(upload.created_at).getTime();
  return age < 60 * 60 * 1000; // within 1 hour
}

function OsmModules() {
  const { modules, uploads } = useLoaderData<LoaderData>();
  const navigate = useNavigate();

  const [deleteTarget, setDeleteTarget] = useState<OsmModule | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const recentUploads = uploads.filter(isRecentUpload);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleteError(null);
    try {
      await deleteOsmModule(deleteTarget.id);
      navigate(0); // reload page
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to delete module";
      setDeleteError(msg);
      throw err; // re-throw so dialog stays open
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Modules" },
        ]}
        title="OS Modules"
        description="Operating system module packages for PXE provisioning"
        actions={
          <Button onClick={() => navigate("/osm/upload")}>
            + Upload Module
          </Button>
        }
      />

      {/* Modules table */}
      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Name", "Version", "Author", "OS Count", "Source", ""] as const).map(
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
            {modules.length === 0 ? (
              <tr>
                <td colSpan={6}>
                  <EmptyState
                    icon={Package}
                    title="No OS modules installed"
                    description="Upload an OS module archive to get started."
                    action={{
                      label: "+ Upload Module",
                      onClick: () => navigate("/osm/upload"),
                    }}
                  />
                </td>
              </tr>
            ) : (
              modules.map((mod, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={mod.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    {/* Name */}
                    <td className="px-3 py-2 text-xs text-text-primary font-semibold">
                      <button
                        onClick={() => navigate(`/osm/${mod.id}`)}
                        className="text-text-primary hover:text-accent transition-colors cursor-pointer"
                        aria-label={`View module ${mod.name}`}
                      >
                        {mod.name}
                      </button>
                    </td>

                    {/* Version */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {mod.version || "—"}
                    </td>

                    {/* Author */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {mod.author || "—"}
                    </td>

                    {/* OS Count */}
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {mod.os_count}
                    </td>

                    {/* Source */}
                    <td className="px-3 py-2">
                      {sourceBadge(mod.source)}
                    </td>

                    {/* Actions */}
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-3">
                        <button
                          onClick={() => navigate(`/osm/${mod.id}`)}
                          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                          aria-label={`View module ${mod.name}`}
                        >
                          view
                        </button>
                        {mod.source === "uploaded" && mod.archive_path && (
                          <a
                            href={getOsmModuleExportUrl(mod.id)}
                            className="text-xs text-accent hover:text-accent-hover transition-colors"
                            aria-label={`Export module ${mod.name}`}
                          >
                            export
                          </a>
                        )}
                        {!mod.is_default && (
                          <button
                            onClick={() => {
                              setDeleteError(null);
                              setDeleteTarget(mod);
                            }}
                            className="text-xs text-status-broken hover:text-status-broken/80 transition-colors cursor-pointer"
                            aria-label={`Delete module ${mod.name}`}
                          >
                            delete
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      {/* Recent Uploads section */}
      {recentUploads.length > 0 && (
        <div className="mt-6">
          <h2 className="text-sm font-semibold text-text-secondary uppercase tracking-[0.5px] mb-2">
            Recent Uploads
          </h2>
          <div className="border border-border">
            {recentUploads.map((upload, idx) => {
              const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
              const hasProgress =
                upload.status === "uploading" &&
                upload.total_bytes != null &&
                upload.total_bytes > 0;
              const progress = hasProgress
                ? Math.round((upload.received_bytes / upload.total_bytes!) * 100)
                : null;

              return (
                <div
                  key={upload.id}
                  className={`${rowBg} border-b border-border-muted last:border-b-0 px-3 py-2 flex items-center gap-3`}
                >
                  <span className="text-xs text-text-primary flex-1 min-w-0 truncate">
                    {upload.filename}
                  </span>
                  {uploadStatusBadge(upload.status)}
                  {progress !== null && (
                    <span className="text-xs text-text-muted">{progress}%</span>
                  )}
                  {upload.error_message && (
                    <span className="text-xs text-status-broken truncate max-w-xs">
                      {upload.error_message}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Delete confirmation dialog */}
      {deleteTarget && (
        <DeleteConfirmationDialog
          open={deleteTarget !== null}
          onOpenChange={(open) => {
            if (!open) {
              setDeleteTarget(null);
              setDeleteError(null);
            }
          }}
          title="Delete OS Module"
          description={`Are you sure you want to delete this module? This action cannot be undone.`}
          itemName={deleteTarget.name}
          warningMessage={deleteError ?? undefined}
          onConfirm={handleDelete}
        />
      )}
    </div>
  );
}

export default OsmModules;
