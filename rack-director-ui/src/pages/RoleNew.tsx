import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import DiskLayoutEditor from "@/components/roles/disk-layout-editor";
import {
  OsVariableForm,
  findOsByKey,
  buildDefaultValues,
  mergeOsValues,
  buildSubmitConfig,
  validateRequiredVars,
} from "@/components/roles/os-variable-form";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { selectClassName } from "@/components/roles/styles";
import {
  createRole,
  getAllOsmOperatingSystems,
  getOsmModules,
  ValidationError,
  type DiskLayout,
  type FirmwareMode,
  type PartitionConfig,
  type OsmOperatingSystem,
  type OsmModule,
} from "@/lib/client";

function defaultDiskLayoutForFirmwareMode(mode: FirmwareMode | undefined): DiskLayout {
  const esp: PartitionConfig = {
    label: "efi",
    size: "300MiB",
    filesystem: "vfat",
    mount_point: "/boot/efi",
    flags: ["esp"],
  };
  const biosGrub: PartitionConfig = {
    label: "bios_grub",
    size: "1MiB",
    flags: ["bios_grub"],
  };
  const root: PartitionConfig = {
    label: "root",
    size: "rest",
    filesystem: "ext4",
    mount_point: "/",
  };
  const partitions =
    mode === "uefi"
      ? [esp, root]
      : mode === "bios"
      ? [biosGrub, root]
      : [esp, biosGrub, root]; // undefined = any
  return { disks: [{ device: "ROOT", partition_table: "gpt", partitions }] };
}

function isDiskLayoutEmpty(layout: DiskLayout): boolean {
  return layout.disks.length === 0;
}

function RoleNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [osmModule, setOsmModule] = useState("");
  const [osName, setOsName] = useState("");
  const [osRelease, setOsRelease] = useState("");
  const [osArch, setOsArch] = useState("x86-64");
  const [diskLayout, setDiskLayout] = useState<DiskLayout>(() =>
    defaultDiskLayoutForFirmwareMode(undefined)
  );
  const [firmwareMode, setFirmwareMode] = useState<FirmwareMode | undefined>(undefined);
  const [osConfigValues, setOsConfigValues] = useState<Record<string, unknown>>({});
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { fieldErrors, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  const [osmOsList, setOsmOsList] = useState<OsmOperatingSystem[]>([]);
  const [osmModules, setOsmModules] = useState<OsmModule[]>([]);
  const [loadingOs, setLoadingOs] = useState(true);
  const [selectedOsKey, setSelectedOsKey] = useState<string>("");
  const [availableArchs, setAvailableArchs] = useState<string[]>([]);

  useEffect(() => {
    // Only seed the layout when firmwareMode changes if the layout is still empty
    if (isDiskLayoutEmpty(diskLayout)) {
      setDiskLayout(defaultDiskLayoutForFirmwareMode(firmwareMode));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firmwareMode]);

  useEffect(() => {
    const fetchOs = async () => {
      try {
        const [osList, modules] = await Promise.all([
          getAllOsmOperatingSystems(),
          getOsmModules(),
        ]);
        const enabled = osList.filter((os) => !os.disabled);
        setOsmOsList(enabled);
        setOsmModules(modules);
        if (enabled.length > 0) {
          const first = enabled[0];
          const firstArch = first.config.architectures[0]?.arch || "x86-64";
          const moduleName = modules.find((m) => m.id === first.module_id)?.name || "";
          const key = `${moduleName}|${first.name}|${first.release}`;
          setSelectedOsKey(key);
          setOsmModule(moduleName);
          setOsName(first.name);
          setOsRelease(first.release);
          setOsArch(firstArch);
          setAvailableArchs(first.config.architectures.map((a) => a.arch));
          setOsConfigValues(buildDefaultValues(first.config.template_variables));
        }
      } catch (err) {
        setError("Failed to load operating systems");
      } finally {
        setLoadingOs(false);
      }
    };
    fetchOs();
  }, []);

  const matchedOs = findOsByKey(selectedOsKey, osmOsList, osmModules);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    clearAllErrors();

    const templateVars = matchedOs?.config.template_variables ?? [];

    const missing = validateRequiredVars(templateVars, osConfigValues);
    if (missing.length > 0) {
      setError(`Required fields missing: ${missing.join(", ")}`);
      return;
    }

    const parsedConfig = buildSubmitConfig(osConfigValues, templateVars);

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

              {/* Operating System Selection */}
              {loadingOs ? (
                <div className="space-y-1">
                  <Label className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Operating System *
                  </Label>
                  <div className="h-8 px-3 flex items-center bg-bg-base border border-border text-xs text-text-muted">
                    Loading operating systems...
                  </div>
                </div>
              ) : osmOsList.length === 0 ? (
                <div className="space-y-1">
                  <Label className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Operating System *
                  </Label>
                  <div className="px-3 py-2 border border-border bg-bg-base text-xs text-text-muted">
                    No operating systems available. Upload an OS module first under{" "}
                    <a href="/osm" className="text-accent underline">
                      Operating System Modules
                    </a>
                    .
                  </div>
                </div>
              ) : (
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div className="space-y-1">
                    <Label htmlFor="os_select" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                      Operating System *
                    </Label>
                    <select
                      id="os_select"
                      value={selectedOsKey}
                      onChange={(e) => {
                        const key = e.target.value;
                        setSelectedOsKey(key);
                        clearFieldError("osm_module");
                        clearFieldError("os_name");
                        clearFieldError("os_release");
                        clearFieldError("os_arch");
                        const [mod, , release] = key.split("|");
                        const os = findOsByKey(key, osmOsList, osmModules);
                        if (os) {
                          const archs = os.config.architectures.map((a) => a.arch);
                          setOsmModule(mod);
                          setOsName(os.name);
                          setOsRelease(release);
                          setAvailableArchs(archs);
                          setOsArch(archs[0] || "x86-64");
                          setOsConfigValues((prev) =>
                            mergeOsValues(prev, os.config.template_variables)
                          );
                        }
                      }}
                      aria-invalid={
                        !!(fieldErrors["osm_module"] || fieldErrors["os_name"] || fieldErrors["os_release"])
                      }
                      className={selectClassName}
                    >
                      {osmOsList.map((os) => {
                        const moduleName = osmModules.find((m) => m.id === os.module_id)?.name || "";
                        const key = `${moduleName}|${os.name}|${os.release}`;
                        return (
                          <option key={key} value={key}>
                            {os.name} {os.release} ({moduleName})
                          </option>
                        );
                      })}
                    </select>
                    <FormFieldError
                      error={fieldErrors["osm_module"] || fieldErrors["os_name"] || fieldErrors["os_release"]}
                    />
                  </div>

                  <div className="space-y-1">
                    <Label htmlFor="os_arch" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                      Architecture *
                    </Label>
                    <select
                      id="os_arch"
                      value={osArch}
                      onChange={(e) => {
                        setOsArch(e.target.value);
                        clearFieldError("os_arch");
                      }}
                      aria-invalid={!!fieldErrors["os_arch"]}
                      className={selectClassName}
                    >
                      {availableArchs.map((arch) => (
                        <option key={arch} value={arch}>
                          {arch}
                        </option>
                      ))}
                    </select>
                    <FormFieldError error={fieldErrors["os_arch"]} />
                  </div>
                </div>
              )}

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
                firmwareMode={firmwareMode}
              />
            </div>
          </div>

          {/* Config Template card */}
          <div className="border border-border bg-bg-surface">
            <div className="px-4 py-3 border-b border-border">
              <span className="text-sm font-semibold text-text-primary">Configuration Template</span>
            </div>
            <div className="px-4 py-4">
              {loadingOs ? (
                <p className="text-xs text-text-muted">Loading...</p>
              ) : (
                <OsVariableForm
                  variables={matchedOs?.config.template_variables ?? []}
                  values={osConfigValues}
                  onChange={setOsConfigValues}
                />
              )}
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
