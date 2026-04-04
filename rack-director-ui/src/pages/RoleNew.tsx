import { useState, useEffect } from "react";
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
  getOperatingSystems,
  ValidationError,
  type DiskLayout,
  type FirmwareMode,
  type OperatingSystem,
} from "@/lib/client";

function RoleNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [osId, setOsId] = useState<number | null>(null);
  const [diskLayout, setDiskLayout] = useState<DiskLayout>({ disks: [] });
  const [firmwareMode, setFirmwareMode] = useState<FirmwareMode | undefined>(undefined);
  const [configTemplate, setConfigTemplate] = useState("");
  const [operatingSystems, setOperatingSystems] = useState<OperatingSystem[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadingOs, setLoadingOs] = useState(true);
  const { fieldErrors, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  useEffect(() => {
    const fetchOperatingSystems = async () => {
      try {
        const osList = await getOperatingSystems();
        setOperatingSystems(osList);
        if (osList.length > 0 && !osId) {
          setOsId(osList[0].id!);
        }
      } catch (err) {
        setError("Failed to load operating systems");
      } finally {
        setLoadingOs(false);
      }
    };
    fetchOperatingSystems();
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    clearAllErrors();

    if (!osId) {
      setError("Please select an operating system");
      return;
    }

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
        os_id: osId,
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

  if (loadingOs) {
    return (
      <div className="text-xs text-text-muted p-4">Loading...</div>
    );
  }

  if (operatingSystems.length === 0) {
    return (
      <div>
        <PageHeader
          breadcrumbs={[
            { label: "Dashboard", href: "/" },
            { label: "Roles", href: "/roles" },
            { label: "New Role" },
          ]}
          title="Create Role"
        />
        <div style={{ maxWidth: 700 }}>
          <div className="border border-border bg-bg-surface p-4">
            <p className="text-xs text-text-secondary text-center mb-3">
              No operating systems available. Please create an operating system first.
            </p>
            <div className="flex justify-center">
              <Button onClick={() => navigate("/operating-systems/new")}>
                Create Operating System
              </Button>
            </div>
          </div>
        </div>
      </div>
    );
  }

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

              {/* OS + Firmware row */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div className="space-y-1">
                  <Label htmlFor="os" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Operating System *
                  </Label>
                  <select
                    id="os"
                    value={osId ?? ""}
                    onChange={(e) => setOsId(parseInt(e.target.value))}
                    className={selectClassName}
                    required
                  >
                    {operatingSystems.map((os) => (
                      <option key={os.id} value={os.id!}>
                        {os.name} {os.version}
                      </option>
                    ))}
                  </select>
                </div>

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
