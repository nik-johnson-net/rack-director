import { useEffect, useState } from "react";
import { FileCode } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { CloudConfigEditor } from "@/components/cloud-config/cloud-config-editor";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import {
  getGlobalCloudConfig,
  updateGlobalCloudConfig,
  ValidationError,
} from "@/lib/client";

/**
 * Edit the global cloud-config layer applied to every device whose OS supports cloud-init.
 * This is where fleet-wide settings such as root SSH keys belong.
 */
export function GlobalCloudConfigCard() {
  const [value, setValue] = useState("");
  const [updatedAt, setUpdatedAt] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const { getError, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  useEffect(() => {
    getGlobalCloudConfig()
      .then((config) => {
        setValue(config.cloud_config ?? "");
        setUpdatedAt(config.updated_at);
      })
      .catch(() => setError("Failed to load global cloud-config"))
      .finally(() => setLoading(false));
  }, []);

  const save = async (next: string | null) => {
    setSaving(true);
    setError(null);
    setSaved(false);
    clearAllErrors();
    try {
      const config = await updateGlobalCloudConfig(next);
      setValue(config.cloud_config ?? "");
      setUpdatedAt(config.updated_at);
      setSaved(true);
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
      } else {
        setError(err instanceof Error ? err.message : "Failed to save global cloud-config");
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card style={{ maxWidth: 700 }}>
      <CardHeader>
        <div>
          <CardTitle className="flex items-center gap-2">
            <FileCode className="h-4 w-4 text-text-secondary" />
            Global Cloud-Config
          </CardTitle>
          <CardDescription className="text-xs">
            Applied on first boot to every device whose OS supports cloud-init, before the
            role's own cloud-config. Put fleet-wide settings such as root SSH keys here.
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {loading ? (
          <p className="text-xs text-text-muted">Loading...</p>
        ) : (
          <>
            <CloudConfigEditor
              id="global_cloud_config"
              value={value}
              onChange={(next) => {
                setValue(next);
                setSaved(false);
                clearFieldError("cloud_config");
              }}
              error={getError("cloud_config")}
              disabled={saving}
            />
            {error && (
              <div className="px-3 py-2 border border-error-border bg-error-bg text-status-broken text-xs">
                {error}
              </div>
            )}
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs text-text-muted">
                {saved
                  ? "Saved."
                  : updatedAt
                    ? `Last updated ${new Date(updatedAt).toLocaleString()}`
                    : "Not set"}
              </span>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  disabled={saving || (!value && !updatedAt)}
                  onClick={() => save(null)}
                >
                  Clear
                </Button>
                <Button type="button" disabled={saving} onClick={() => save(value.trim() ? value : null)}>
                  {saving ? "Saving..." : "Save"}
                </Button>
              </div>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
