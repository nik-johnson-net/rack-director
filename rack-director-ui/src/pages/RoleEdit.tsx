import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import DiskLayoutEditor from "@/components/roles/disk-layout-editor";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { selectClassName } from "@/components/roles/styles";
import { Trash2 } from "lucide-react";
import {
  updateRole,
  deleteRole,
  getRoleDevices,
  getOperatingSystems,
  ValidationError,
  type RoleWithOs,
  type DiskLayout,
  type FirmwareMode,
  type OperatingSystem,
} from "@/lib/client";

function RoleEdit() {
  const initialData = useLoaderData<RoleWithOs>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const roleId = parseInt(params.id!);

  const [data, setData] = useState(initialData);
  const [name, setName] = useState(data.name);
  const [description, setDescription] = useState(data.description || "");
  const [osId, setOsId] = useState(data.os_id);
  const [diskLayout, setDiskLayout] = useState<DiskLayout>(
    data.disk_layout ?? { disks: [] }
  );
  const [firmwareMode, setFirmwareMode] = useState<FirmwareMode | undefined>(
    data.firmware_mode
  );
  const [configTemplate, setConfigTemplate] = useState(
    data.config_template ? JSON.stringify(data.config_template, null, 2) : ""
  );
  const [operatingSystems, setOperatingSystems] = useState<OperatingSystem[]>([]);
  const [assignedDevices, setAssignedDevices] = useState<string[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const { fieldErrors, setErrors, clearAllErrors, clearFieldError } = useFieldErrors();

  useEffect(() => {
    const fetchData = async () => {
      try {
        const [osList, devices] = await Promise.all([
          getOperatingSystems(),
          getRoleDevices(roleId),
        ]);
        setOperatingSystems(osList);
        setAssignedDevices(devices);
      } catch (err) {
        setError("Failed to load data");
      }
    };
    fetchData();
  }, [roleId]);

  useEffect(() => {
    if (saveSuccess) window.scrollTo({ top: 0, behavior: "smooth" });
  }, [saveSuccess]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSaveSuccess(false);
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
      const updated = await updateRole(roleId, {
        name,
        description: description || undefined,
        os_id: osId,
        disk_layout: diskLayout,
        firmware_mode: firmwareMode || undefined,
        clear_firmware_mode: firmwareMode === undefined ? true : undefined,
        config_template: parsedConfig,
      });

      setData({ ...data, ...updated, os_name: data.os_name, os_version: data.os_version });
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

              {/* OS + Firmware row */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div className="space-y-1">
                  <Label htmlFor="os" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Operating System *
                  </Label>
                  <select
                    id="os"
                    value={osId}
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
                  <p className="text-xs text-text-muted">
                    Architectures are inferred from the selected OS
                  </p>
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
