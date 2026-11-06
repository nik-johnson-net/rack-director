import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { getDeviceCloudConfig, type DeviceCloudConfigPreview } from "@/lib/client";

interface CloudConfigPreviewProps {
  uuid: string;
}

/**
 * Read-only view of the merged cloud-config seed (global + role layers) the device will be
 * served on its first boot. Loaded on demand the first time it is expanded.
 */
export function CloudConfigPreview({ uuid }: CloudConfigPreviewProps) {
  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState<DeviceCloudConfigPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = async () => {
    const next = !open;
    setOpen(next);
    // Fetch once; the parent remounts this component when the device's role changes.
    if (!next || preview) return;
    setLoading(true);
    setError(null);
    try {
      setPreview(await getDeviceCloudConfig(uuid));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load cloud-config preview");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="bg-bg-surface border border-border lg:col-span-2">
      <button
        type="button"
        onClick={toggle}
        className="w-full flex items-center gap-2 px-4 py-3 text-left cursor-pointer hover:bg-bg-raised transition-colors"
      >
        {open ? (
          <ChevronDown className="h-4 w-4 text-text-secondary" />
        ) : (
          <ChevronRight className="h-4 w-4 text-text-secondary" />
        )}
        <span className="text-base font-semibold text-text-primary">Cloud-Config</span>
        <span className="text-xs text-text-muted">merged seed served on first boot</span>
      </button>
      {open && (
        <div className="px-4 pb-4 space-y-3">
          {loading && <p className="text-xs text-text-muted">Loading...</p>}
          {error && <p className="text-xs text-status-broken">{error}</p>}
          {preview && !loading && <PreviewBody preview={preview} />}
        </div>
      )}
    </div>
  );
}

function PreviewBody({ preview }: { preview: DeviceCloudConfigPreview }) {
  if (!preview.supported) {
    return (
      <p className="text-xs text-text-secondary">
        This device's role OS does not support cloud-config, so nothing is served.
      </p>
    );
  }
  return (
    <>
      <p className="text-xs text-text-secondary">
        Seed URL: <code className="text-text-primary">{"<rack-director>"}{preview.seed_path}</code>{" "}
        (served only while the device is at its first boot)
      </p>
      <SeedFile name="meta-data" content={preview.meta_data} />
      <SeedFile name="user-data" content={preview.user_data} />
    </>
  );
}

function SeedFile({ name, content }: { name: string; content: string | null }) {
  return (
    <div className="space-y-1">
      <div className="text-xs text-text-secondary uppercase tracking-[0.5px]">{name}</div>
      <pre className="bg-bg-base border border-border px-3 py-2 text-xs text-text-primary overflow-x-auto whitespace-pre">
        {content ?? ""}
      </pre>
    </div>
  );
}
