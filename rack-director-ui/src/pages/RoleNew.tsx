import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import DiskLayoutEditor from "@/components/roles/disk-layout-editor";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { selectClassName } from "@/components/roles/styles";
import {
  createRole,
  ValidationError,
  type DiskLayout,
  type FirmwareMode,
} from "@/lib/client";

function RoleNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [osmModule, setOsmModule] = useState("");
  const [osName, setOsName] = useState("");
  const [osRelease, setOsRelease] = useState("");
  const [osArch, setOsArch] = useState("x86-64");
  const [diskLayout, setDiskLayout] = useState<DiskLayout>({ disks: [] });
  const [firmwareMode, setFirmwareMode] = useState<FirmwareMode | undefined>(undefined);
  const [configTemplate, setConfigTemplate] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { fieldErrors, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    clearAllErrors();

    let parsedConfig = undefined;
    if (configTemplate.trim()) {
      try {
        parsedConfig = JSON.parse(configTemplate);
      } catch (err) {
        setError("Invalid JSON in config template");
        return;
      }
    }

    setIsSubmitting(true);

    try {
      const role = await createRole({
        name,
        description: description || undefined,
        osm_module: osmModule,
        os_name: osName,
        os_release: osRelease,
        os_arch: osArch,
        disk_layout: diskLayout,
        firmware_mode: firmwareMode || undefined,
        config_template: parsedConfig,
      });

      navigate(`/roles/${role.id}`);
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
        setError("Please fix the validation errors below");
      } else {
        setError(err instanceof Error ? err.message : "Failed to create role");
      }
      setIsSubmitting(false);
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Roles", href: "/roles" },
          { label: "New Role" },
        ]}
        title="Create Role"
        description="Define a provisioning role with OS, disk layout, and configuration"
      />

      <form onSubmit={handleSubmit}>
        <div style={{ maxWidth: 700 }} className="space-y-4">
          {/* General card */}
          <div className="border border-border bg-bg-surface">
            <div className="px-4 py-3 border-b border-border">
              <span className="text-sm font-semibold text-text-primary">General</span>
            </div>
            <div className="px-4 py-4 space-y-4">
              {/* Name */}
              <div className="space-y-1">
                <Label htmlFor="name" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  Name *
                </Label>
                <Input
                  id="name"
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    clearFieldError("name");
                  }}
                  placeholder="e.g., k8s-worker, storage-node"
                  aria-invalid={!!fieldErrors["name"]}
                  className="h-8 text-xs"
                />
                <FormFieldError error={fieldErrors["name"]} />
              </div>

              {/* Description */}
              <div className="space-y-1">
                <Label htmlFor="description" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  Description
                </Label>
                <Input
                  id="description"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="Optional description"
                  className="h-8 text-xs"
                />
              </div>

              {/* OSM Module */}
              <div className="space-y-1">
                <Label htmlFor="osm_module" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  OSM Module *
                </Label>
                <Input
                  id="osm_module"
                  value={osmModule}
                  onChange={(e) => {
                    setOsmModule(e.target.value);
                    clearFieldError("osm_module");
                  }}
                  placeholder="e.g., centos"
                  aria-invalid={!!fieldErrors["osm_module"]}
                  className="h-8 text-xs"
                />
                <FormFieldError error={fieldErrors["osm_module"]} />
              </div>

              {/* OS Name / Release / Arch + Firmware row */}
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="space-y-1">
                  <Label htmlFor="os_name" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    OS Name *
                  </Label>
                  <Input
                    id="os_name"
                    value={osName}
                    onChange={(e) => {
                      setOsName(e.target.value);
                      clearFieldError("os_name");
                    }}
                    placeholder="e.g., centos"
                    aria-invalid={!!fieldErrors["os_name"]}
                    className="h-8 text-xs"
                  />
                  <FormFieldError error={fieldErrors["os_name"]} />
                </div>

                <div className="space-y-1">
                  <Label htmlFor="os_release" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Release *
                  </Label>
                  <Input
                    id="os_release"
                    value={osRelease}
                    onChange={(e) => {
                      setOsRelease(e.target.value);
                      clearFieldError("os_release");
                    }}
                    placeholder="e.g., 10"
                    aria-invalid={!!fieldErrors["os_release"]}
                    className="h-8 text-xs"
                  />
                  <FormFieldError error={fieldErrors["os_release"]} />
                </div>

                <div className="space-y-1">
                  <Label htmlFor="os_arch" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Architecture *
                  </Label>
                  <Input
                    id="os_arch"
                    value={osArch}
                    onChange={(e) => {
                      setOsArch(e.target.value);
                      clearFieldError("os_arch");
                    }}
                    placeholder="e.g., x86-64"
                    aria-invalid={!!fieldErrors["os_arch"]}
                    className="h-8 text-xs"
                  />
                  <FormFieldError error={fieldErrors["os_arch"]} />
                </div>
              </div>

              {/* Firmware Mode */}
              <div className="space-y-1">
                <Label htmlFor="firmware_mode" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  Firmware Mode
                </Label>
                <select
                  id="firmware_mode"
                  value={firmwareMode ?? ""}
                  onChange={(e) =>
                    setFirmwareMode((e.target.value as FirmwareMode) || undefined)
                  }
                  className={selectClassName}
                >
                  <option value="">Any</option>
                  <option value="uefi">UEFI</option>
                  <option value="bios">BIOS</option>
                </select>
                <p className="text-xs text-text-muted">
                  Constrains which devices can be assigned this role
                </p>
              </div>
            </div>
          </div>

          {/* Disk Layout card */}
          <div className="border border-border bg-bg-surface">
            <div className="px-4 py-3 border-b border-border flex items-center justify-between">
              <span className="text-sm font-semibold text-text-primary">Disk Layout</span>
              <button
                type="button"
                onClick={() => {
                  // Trigger add disk in the editor via a custom event isn't ideal,
                  // so we rely on the "+ Add Device" inside the editor empty state.
                  // This button is shown here for visual consistency per spec.
                }}
                className="text-xs text-text-secondary border border-border px-2 py-1 hover:border-accent hover:text-text-primary transition-colors rounded-sm cursor-not-allowed opacity-50"
                title="Use the '+ Add Device' button below when no disks are defined"
              >
                + Add Device
              </button>
            </div>
            <div className="px-4 py-4">
              <DiskLayoutEditor
                value={diskLayout}
                onChange={setDiskLayout}
                errors={fieldErrors}
                onClearError={clearFieldError}
              />
            </div>
          </div>

          {/* Config Template card */}
          <div className="border border-border bg-bg-surface">
            <div className="px-4 py-3 border-b border-border">
              <span className="text-sm font-semibold text-text-primary">Configuration Template</span>
            </div>
            <div className="px-4 py-4">
              <div className="space-y-1">
                <Label htmlFor="config" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  JSON Configuration
                </Label>
                <textarea
                  id="config"
                  value={configTemplate}
                  onChange={(e) => setConfigTemplate(e.target.value)}
                  placeholder={'{\n  "packages": ["nginx", "postgresql"],\n  "custom_setting": "value"\n}'}
                  rows={8}
                  className="w-full bg-bg-base border border-border text-text-primary text-xs px-3 py-2 font-mono focus:outline-none focus:border-accent resize-y rounded-sm placeholder:text-text-muted"
                />
                <p className="text-xs text-text-muted">
                  Optional JSON accessible in install scripts via template variables
                </p>
              </div>
            </div>
          </div>

          {/* Error banner */}
          {error && (
            <div className="px-3 py-2 border border-error-border bg-error-bg text-status-broken text-xs">
              {error}
            </div>
          )}

          {/* Actions */}
          <div className="flex gap-2 justify-end">
            <Button
              type="button"
              variant="outline"
              onClick={() => navigate("/roles")}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? "Creating..." : "Create Role"}
            </Button>
          </div>
        </div>
      </form>
    </div>
  );
}

export default RoleNew;
