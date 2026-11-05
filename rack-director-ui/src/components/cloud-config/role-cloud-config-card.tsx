import { Link } from "react-router";
import { CloudConfigEditor } from "@/components/cloud-config/cloud-config-editor";
import type { OsmOperatingSystem } from "@/lib/client";

interface RoleCloudConfigCardProps {
  value: string;
  onChange: (value: string) => void;
  error?: string;
  /** The role's selected OS; `undefined` while operating systems are loading. */
  os: OsmOperatingSystem | undefined;
  loading: boolean;
}

/** The role's cloud-config layer, applied on first boot after the global layer. */
export function RoleCloudConfigCard({ value, onChange, error, os, loading }: RoleCloudConfigCardProps) {
  const supported = !!os?.config.cloud_init;
  // Keep a stale value editable so it can be cleared after switching to an unsupported OS.
  const disabled = !loading && !supported && !value;

  return (
    <div className="border border-border bg-bg-surface">
      <div className="px-4 py-3 border-b border-border">
        <span className="text-sm font-semibold text-text-primary">Cloud-Config</span>
      </div>
      <div className="px-4 py-4 space-y-3">
        <p className="text-xs text-text-secondary">
          Applied by cloud-init on the device's first boot, merged over the{" "}
          <Link to="/settings" className="text-accent hover:text-accent-hover">
            global cloud-config
          </Link>
          . Provisioning completes once it has been applied.
        </p>
        {!loading && !supported && (
          <div className="px-3 py-2 border border-warn-border bg-warn-bg text-status-unprovisioned text-xs">
            {os ? `${os.name} ${os.release}` : "The selected OS"} does not support cloud-config
            (its OSM does not declare <code>cloud_init</code>).
            {value.trim() && " Provisioning devices with this role is refused until it is cleared."}
          </div>
        )}
        <CloudConfigEditor
          id="cloud_config"
          value={value}
          onChange={onChange}
          error={error}
          disabled={disabled}
        />
      </div>
    </div>
  );
}
