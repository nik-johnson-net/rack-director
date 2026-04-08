import { useState, useEffect, useRef } from "react";
import { Link } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { uploadOsm, getOsmUpload, type OsmUpload as OsmUploadType } from "@/lib/client";
import { Upload, FileArchive, CheckCircle, XCircle } from "lucide-react";

const labelClass = "text-xs text-text-secondary uppercase tracking-[0.5px]";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

type StatusColor = {
  bg: string;
  text: string;
  dot: string;
};

function getStatusColors(status: OsmUploadType["status"]): StatusColor {
  switch (status) {
    case "uploading":
      return { bg: "bg-status-transitioning-bg", text: "text-status-transitioning", dot: "bg-status-transitioning" };
    case "validating":
      return { bg: "bg-status-new-bg", text: "text-status-new", dot: "bg-status-new" };
    case "extracting":
      return { bg: "bg-status-unprovisioned-bg", text: "text-status-unprovisioned", dot: "bg-status-unprovisioned" };
    case "complete":
      return { bg: "bg-status-provisioned-bg", text: "text-status-provisioned", dot: "bg-status-provisioned" };
    case "failed":
      return { bg: "bg-status-broken-bg", text: "text-status-broken", dot: "bg-status-broken" };
  }
}

function StatusBadge({ status }: { status: OsmUploadType["status"] }) {
  const colors = getStatusColors(status);
  return (
    <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 text-xs font-semibold uppercase tracking-[0.5px] ${colors.bg} ${colors.text}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${colors.dot}`} />
      {status}
    </span>
  );
}

function SectionCard({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border bg-bg-surface">
      <div className="px-4 py-3 border-b border-border">
        <span className="text-sm font-semibold text-text-primary">{title}</span>
        {subtitle && (
          <p className="text-xs text-text-secondary mt-0.5">{subtitle}</p>
        )}
      </div>
      <div className="px-4 py-4 space-y-4">{children}</div>
    </div>
  );
}

function OsmUpload() {
  const [file, setFile] = useState<File | null>(null);
  const [uploading, setUploading] = useState(false);
  const [upload, setUpload] = useState<OsmUploadType | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Poll for upload status when we have an upload in progress
  useEffect(() => {
    if (!upload) return;

    const isTerminal = upload.status === "complete" || upload.status === "failed";
    if (isTerminal) {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    // Start polling
    intervalRef.current = setInterval(async () => {
      try {
        const updated = await getOsmUpload(upload.id);
        setUpload(updated);
        if (updated.status === "complete" || updated.status === "failed") {
          if (intervalRef.current) {
            clearInterval(intervalRef.current);
            intervalRef.current = null;
          }
        }
      } catch {
        // Silently ignore polling errors; will retry on next interval
      }
    }, 2000);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [upload?.id, upload?.status]);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const selected = e.target.files?.[0] ?? null;
    setFile(selected);
    setError(null);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!file) return;

    setUploading(true);
    setError(null);

    try {
      const result = await uploadOsm(file);
      setUpload(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start upload");
      setUploading(false);
    }
  };

  const handleReset = () => {
    setFile(null);
    setUpload(null);
    setError(null);
    setUploading(false);
    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  };

  // Calculate progress percentage
  const progressPercent =
    upload && upload.total_bytes && upload.total_bytes > 0
      ? Math.round((upload.received_bytes / upload.total_bytes) * 100)
      : null;

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Modules", href: "/osm" },
          { label: "Upload" },
        ]}
        title="Upload OS Module"
        description="Upload an OSM archive (.tar.zst or .osm) containing operating system definitions"
      />

      <div style={{ maxWidth: 600 }} className="space-y-4">
        {/* File Selection State */}
        {!uploading && !upload && (
          <form onSubmit={handleSubmit}>
            <SectionCard
              title="Select Archive"
              subtitle="Upload an OSM archive (.tar.zst or .osm) containing operating system definitions"
            >
              <div className="space-y-3">
                {/* File input area */}
                <div>
                  <label className={labelClass}>Archive File</label>
                  <div
                    className="mt-1 border border-border border-dashed bg-bg-base p-6 flex flex-col items-center justify-center gap-2 cursor-pointer hover:border-accent transition-colors"
                    onClick={() => fileInputRef.current?.click()}
                    onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") fileInputRef.current?.click(); }}
                    tabIndex={0}
                    role="button"
                    aria-label="Select archive file"
                  >
                    <FileArchive className="h-8 w-8 text-text-muted" />
                    {file ? (
                      <div className="text-center">
                        <p className="text-xs font-mono text-text-primary">{file.name}</p>
                        <p className="text-xs text-text-secondary mt-0.5">{formatBytes(file.size)}</p>
                      </div>
                    ) : (
                      <div className="text-center">
                        <p className="text-xs text-text-secondary">Click to select a .tar.zst or .osm file</p>
                        <p className="text-xs text-text-muted mt-0.5">or drag and drop</p>
                      </div>
                    )}
                  </div>
                  <input
                    ref={fileInputRef}
                    type="file"
                    accept=".tar.zst,.zst,.osm"
                    onChange={handleFileChange}
                    className="sr-only"
                    aria-label="Archive file input"
                  />
                </div>

                {/* Selected file summary */}
                {file && (
                  <div className="border border-border bg-bg-raised px-3 py-2 flex items-center gap-2">
                    <FileArchive className="h-3.5 w-3.5 text-accent flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <p className="text-xs font-mono text-text-primary truncate">{file.name}</p>
                      <p className="text-xs text-text-muted">{formatBytes(file.size)}</p>
                    </div>
                    <button
                      type="button"
                      onClick={(e) => { e.stopPropagation(); handleReset(); }}
                      className="text-xs text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                      aria-label="Remove selected file"
                    >
                      Remove
                    </button>
                  </div>
                )}
              </div>

              {error && (
                <div className="px-3 py-2 border border-status-broken bg-status-broken-bg text-status-broken text-xs">
                  {error}
                </div>
              )}
            </SectionCard>

            <div className="flex gap-2 mt-4 pb-8">
              <Button type="submit" disabled={!file}>
                <Upload className="h-3.5 w-3.5 mr-1.5" />
                Upload
              </Button>
              <Link to="/osm">
                <Button type="button" variant="secondary">
                  Cancel
                </Button>
              </Link>
            </div>
          </form>
        )}

        {/* Upload in progress / processing state */}
        {(uploading || (upload && upload.status !== "complete" && upload.status !== "failed")) && (
          <SectionCard title="Upload Progress">
            <div className="space-y-4">
              {/* Filename */}
              <div className="space-y-0.5">
                <p className={labelClass}>File</p>
                <p className="text-xs font-mono text-text-primary">{file?.name ?? upload?.filename ?? "—"}</p>
              </div>

              {/* Status */}
              <div className="space-y-0.5">
                <p className={labelClass}>Status</p>
                {upload ? (
                  <StatusBadge status={upload.status} />
                ) : (
                  <StatusBadge status="uploading" />
                )}
              </div>

              {/* Progress bar */}
              {upload && (
                <div className="space-y-1.5">
                  <p className={labelClass}>Progress</p>
                  <div className="space-y-1">
                    {/* Track */}
                    <div className="h-2 bg-bg-raised border border-border overflow-hidden">
                      {progressPercent !== null ? (
                        <div
                          className="h-full bg-accent transition-all duration-300"
                          style={{ width: `${progressPercent}%` }}
                        />
                      ) : (
                        /* Indeterminate animation when total is unknown */
                        <div className="h-full w-1/3 bg-accent animate-pulse" />
                      )}
                    </div>
                    {/* Bytes label */}
                    <div className="flex justify-between items-center">
                      <span className="text-xs text-text-muted">
                        {formatBytes(upload.received_bytes)} received
                        {upload.total_bytes ? ` / ${formatBytes(upload.total_bytes)}` : ""}
                      </span>
                      {progressPercent !== null && (
                        <span className="text-xs font-semibold text-text-primary">{progressPercent}%</span>
                      )}
                    </div>
                  </div>
                </div>
              )}

              {/* Loading indicator when upload object not yet returned */}
              {!upload && (
                <div className="flex items-center gap-2 text-xs text-text-secondary">
                  <div className="h-1.5 w-1.5 rounded-full bg-accent animate-pulse" />
                  Initiating upload...
                </div>
              )}
            </div>
          </SectionCard>
        )}

        {/* Complete state */}
        {upload?.status === "complete" && (
          <SectionCard title="Upload Complete">
            <div className="space-y-4">
              <div className="flex items-start gap-3">
                <CheckCircle className="h-5 w-5 text-status-provisioned flex-shrink-0 mt-0.5" />
                <div className="space-y-0.5">
                  <p className="text-sm font-semibold text-text-primary">Module uploaded successfully</p>
                  <p className="text-xs font-mono text-text-secondary">{file?.name ?? upload.filename}</p>
                </div>
              </div>

              <div className="flex gap-2 pt-2">
                {upload.module_id != null && (
                  <Link to={`/osm/${upload.module_id}`}>
                    <Button type="button">
                      View Module &rarr;
                    </Button>
                  </Link>
                )}
                <Button type="button" variant="secondary" onClick={handleReset}>
                  Upload Another
                </Button>
              </div>
            </div>
          </SectionCard>
        )}

        {/* Failed state */}
        {upload?.status === "failed" && (
          <SectionCard title="Upload Failed">
            <div className="space-y-4">
              <div className="flex items-start gap-3">
                <XCircle className="h-5 w-5 text-status-broken flex-shrink-0 mt-0.5" />
                <div className="space-y-0.5">
                  <p className="text-sm font-semibold text-text-primary">Upload failed</p>
                  {upload.error_message && (
                    <p className="text-xs text-status-broken font-mono">{upload.error_message}</p>
                  )}
                </div>
              </div>

              <div className="flex gap-2 pt-2">
                <Button type="button" onClick={handleReset}>
                  Try Again
                </Button>
                <Link to="/osm">
                  <Button type="button" variant="secondary">
                    Back to Modules
                  </Button>
                </Link>
              </div>
            </div>
          </SectionCard>
        )}
      </div>
    </div>
  );
}

export default OsmUpload;
