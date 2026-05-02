import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import DiskLayoutEditor from "@/components/roles/disk-layout-editor";
import {
  OsVariableForm,
  findOsByKey,
  mergeOsValues,
  buildSubmitConfig,
  validateRequiredVars,
} from "@/components/roles/os-variable-form";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { selectClassName } from "@/components/roles/styles";
import { Trash2 } from "lucide-react";
import {
  updateRole,
  deleteRole,
  getRoleDevices,
  getAllOsmOperatingSystems,
  getOsmModules,
  ValidationError,
  type Role,
  type DiskLayout,
  type FirmwareMode,
  type OsmOperatingSystem,
  type OsmModule,
} from "@/lib/client";

function RoleEdit() {
  const initialData = useLoaderData<Role>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const roleId = parseInt(params.id!);

  const [data, setData] = useState(initialData);
  const [name, setName] = useState(data.name);
  const [description, setDescription] = useState(data.description || "");
  const [osmModule, setOsmModule] = useState(data.osm_module || "");
  const [osName, setOsName] = useState(data.os_name || "");
  const [osRelease, setOsRelease] = useState(data.os_release || "");
  const [osArch, setOsArch] = useState(data.os_arch || "x86-64");
  const [diskLayout, setDiskLayout] = useState<DiskLayout>(
    data.disk_layout ?? { disks: [] }
  );
  const [firmwareMode, setFirmwareMode] = useState<FirmwareMode | undefined>(
    data.firmware_mode
  );
  const [osConfigValues, setOsConfigValues] = useState<Record<string, unknown>>(
    data.config_template ?? {}
  );
  const [assignedDevices, setAssignedDevices] = useState<string[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const { fieldErrors, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  const [osmOsList, setOsmOsList] = useState<OsmOperatingSystem[]>([]);
  const [osmModules, setOsmModules] = useState<OsmModule[]>([]);
  const [loadingOs, setLoadingOs] = useState(true);
  const [selectedOsKey, setSelectedOsKey] = useState<string>(
    `${data.osm_module || ""}|${data.os_name || ""}|${data.os_release || ""}`
  );
  const [availableArchs, setAvailableArchs] = useState<string[]>([data.os_arch || "x86-64"]);

  useEffect(() => {
    const fetchData = async () => {
      try {
        const [devices, osList, modules] = await Promise.all([
          getRoleDevices(roleId),
          getAllOsmOperatingSystems(),
          getOsmModules(),
        ]);
        setAssignedDevices(devices);
        setOsmModules(modules);

        // Include disabled OS if it matches the current role's OS (so it stays selectable)
        const currentKey = `${data.osm_module || ""}|${data.os_name || ""}|${data.os_release || ""}`;
        const enrichedList = osList.map((os) => {
          const modName = modules.find((m) => m.id === os.module_id)?.name || "";
          const key = `${modName}|${os.name}|${os.release}`;
          return { os, key };
        });
        // Keep enabled + the current OS even if disabled
        const filtered = enrichedList
          .filter(({ os, key }) => !os.disabled || key === currentKey)
          .map(({ os }) => os);
        setOsmOsList(filtered);

        // Determine available archs for current OS
        const currentOs = osList.find((os) => {
          const modName = modules.find((m) => m.id === os.module_id)?.name || "";
          return `${modName}|${os.name}|${os.release}` === currentKey;
        });
        if (currentOs) {
          setAvailableArchs(currentOs.config.architectures.map((a) => a.arch));
          setOsConfigValues((prev) =>
            mergeOsValues(prev, currentOs.config.template_variables)
          );
        }
      } catch (err) {
        setError("Failed to load data");
      } finally {
        setLoadingOs(false);
      }
    };
    fetchData();
  }, [roleId]);

  useEffect(() => {
    if (saveSuccess) window.scrollTo({ top: 0, behavior: "smooth" });
  }, [saveSuccess]);

  const matchedOs = findOsByKey(selectedOsKey, osmOsList, osmModules);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSaveSuccess(false);
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
      const updated = await updateRole(roleId, {
        name,
        description: description || undefined,
        osm_module: osmModule,
        os_name: osName,
        os_release: osRelease,
        os_arch: osArch,
        disk_layout: diskLayout,
        firmware_mode: firmwareMode || undefined,
        clear_firmware_mode: firmwareMode === undefined ? true : undefined,
        config_template: parsedConfig,
      });

      setData({ ...data, ...updated });
      setSaveSuccess(true);
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
        setError("Please fix the validation errors below");
      } else {
        setError(err instanceof Error ? err.message : "Failed to update role");
      }
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async () => {
    await deleteRole(roleId);
    navigate("/roles");
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Roles", href: "/roles" },
          { label: data.name },
        ]}
        title={`Edit Role: ${data.name}`}
        actions={
          <Button variant="danger" onClick={() => setDeleteDialogOpen(true)}>
            <Trash2 className="h-3.5 w-3.5" />
            Delete
          </Button>
        }
      />

      {/* Save success banner */}
      {saveSuccess && (
        <div className="mb-4 px-3 py-2 border border-status-provisioned/30 bg-status-provisioned-bg text-status-provisioned text-xs">
          Role saved successfully.
        </div>
      )}

      {/* Assigned Devices */}
      {assignedDevices.length > 0 && (
        <div className="mb-4 border border-border bg-bg-surface" style={{ maxWidth: 700 }}>
          <div className="px-4 py-3 border-b border-border">
            <span className="text-sm font-semibold text-text-primary">
              Assigned Devices ({assignedDevices.length})
            </span>
          </div>
          <div className="px-4 py-3 flex flex-wrap gap-2">
            {assignedDevices.map((uuid) => (
              <span
                key={uuid}
                className="text-xs font-mono text-text-secondary bg-bg-raised px-2 py-0.5 border border-border-muted rounded-sm"
              >
                {uuid}
              </span>
            ))}
          </div>
        </div>
      )}

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
              ) : (
                <>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <div className="space-y-1">
                      <Label htmlFor="os_select" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                        Operating System *
                      </Label>
                      {osmOsList.length === 0 ? (
                        <div className="h-8 px-3 flex items-center bg-bg-base border border-border text-xs text-text-secondary font-mono">
                          {osName} {osRelease} ({osmModule})
                        </div>
                      ) : (
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
                            const label = os.disabled
                              ? `${os.name} ${os.release} (${moduleName}) [Disabled]`
                              : `${os.name} ${os.release} (${moduleName})`;
                            return (
                              <option key={key} value={key}>
                                {label}
                              </option>
                            );
                          })}
                        </select>
                      )}
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

                </>
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
              {isSubmitting ? "Saving..." : "Save Role"}
            </Button>
          </div>
        </div>
      </form>

      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Role?"
        description="This will permanently delete this role. This action cannot be undone."
        warningMessage={
          assignedDevices.length > 0
            ? `Warning: This role is assigned to ${assignedDevices.length} device(s).`
            : undefined
        }
        onConfirm={handleDelete}
      />
    </div>
  );
}

export default RoleEdit;
