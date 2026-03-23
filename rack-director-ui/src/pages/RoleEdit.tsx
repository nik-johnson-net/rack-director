import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import { FormField, FormTextareaField, FormSelectField } from "@/components/ui/form-field";
import DiskLayoutEditor from "@/components/roles/disk-layout-editor";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import { useFieldErrors } from "@/hooks/useFieldErrors";
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
import { AlertBanner } from "@/components/ui/alert-banner";
import { Trash2 } from "lucide-react";

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
          getRoleDevices(roleId)
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

    // Validate JSON if provided
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
    navigate('/roles');
  };

  return (
    <div className="space-y-4 max-w-4xl">
      <PageHeader
        breadcrumbs={[
          { label: "Roles", href: "/roles" },
          { label: data.name }
        ]}
        title={data.name}
        status={
          <Badge variant="outline">
            {data.os_name} {data.os_version}
          </Badge>
        }
        actions={
          <Button variant="destructive" onClick={() => setDeleteDialogOpen(true)}>
            <Trash2 className="h-4 w-4 mr-2" />
            Delete
          </Button>
        }
      />

      <AlertBanner variant="success" message={saveSuccess ? "Role saved successfully." : null} />
      <AlertBanner variant="error" message={error} />

      {/* Assigned Devices */}
      {assignedDevices.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Assigned Devices</CardTitle>
            <CardDescription>
              {assignedDevices.length} device{assignedDevices.length !== 1 ? 's' : ''} using this role
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-2">
              {assignedDevices.map((uuid) => (
                <Badge key={uuid} variant="secondary" className="font-mono text-xs">
                  {uuid}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Basic Information */}
        <Card>
          <CardHeader>
            <CardTitle>Basic Information</CardTitle>
            <CardDescription>
              Role name, description, and operating system
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <FormField
              id="name"
              label="Name"
              required
              value={name}
              onChange={(val) => {
                setName(val);
                clearFieldError("name");
              }}
              error={fieldErrors["name"]}
              onClearError={() => clearFieldError("name")}
            />

            <FormTextareaField
              id="description"
              label="Description"
              value={description}
              onChange={setDescription}
              rows={2}
            />

            <FormSelectField
              id="os"
              label="Operating System"
              required
              value={osId}
              onChange={(value) => setOsId(parseInt(value))}
              options={operatingSystems.map((os) => ({
                value: os.id!,
                label: `${os.name} ${os.version}`
              }))}
              helperText="Supported architectures are inferred from the selected OS"
            />

            <FormSelectField
              id="firmware_mode"
              label="Firmware Mode"
              value={firmwareMode || ""}
              onChange={(val) => setFirmwareMode((val as FirmwareMode) || undefined)}
              options={[
                { value: "", label: "— No constraint" },
                { value: "bios", label: "BIOS" },
                { value: "uefi", label: "UEFI" },
              ]}
              helperText="If set, only devices with this firmware mode can be assigned this role"
            />
          </CardContent>
        </Card>

        {/* Disk Layout */}
        <Card>
          <CardHeader>
            <CardTitle>Disk Layout</CardTitle>
            <CardDescription>
              Partition scheme for devices with this role
            </CardDescription>
          </CardHeader>
          <CardContent>
            <DiskLayoutEditor
              value={diskLayout}
              onChange={setDiskLayout}
              errors={fieldErrors}
              onClearError={clearFieldError}
            />
          </CardContent>
        </Card>

        {/* Config Template */}
        <Card>
          <CardHeader>
            <CardTitle>Configuration Template</CardTitle>
            <CardDescription>
              Optional JSON configuration available in install scripts
            </CardDescription>
          </CardHeader>
          <CardContent>
            <FormTextareaField
              id="config"
              label="JSON Configuration"
              value={configTemplate}
              onChange={setConfigTemplate}
              placeholder={'{\n  "packages": ["nginx", "postgresql"],\n  "custom_setting": "value"\n}'}
              rows={8}
              inputClassName="font-mono text-sm"
              helperText="This configuration is accessible in install scripts via template variables"
            />
          </CardContent>
        </Card>

        <div className="flex gap-2">
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Saving..." : "Save Changes"}
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => navigate('/roles')}
          >
            Cancel
          </Button>
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
