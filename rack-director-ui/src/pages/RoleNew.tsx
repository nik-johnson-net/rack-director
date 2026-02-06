import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { FormField, FormTextareaField, FormSelectField } from "@/components/ui/form-field";
import PartitionEditor from "@/components/roles/partition-editor";
import { createRole, getOperatingSystems, type Partition, type OperatingSystem } from "@/lib/client";

function RoleNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [osId, setOsId] = useState<number | null>(null);
  const [partitions, setPartitions] = useState<Partition[]>([]);
  const [configTemplate, setConfigTemplate] = useState("");
  const [operatingSystems, setOperatingSystems] = useState<OperatingSystem[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadingOs, setLoadingOs] = useState(true);

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

    if (!osId) {
      setError("Please select an operating system");
      return;
    }

    if (partitions.length === 0) {
      setError("Please add at least one partition");
      return;
    }

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
      const role = await createRole({
        name,
        description: description || undefined,
        os_id: osId,
        disk_layout: { partitions },
        config_template: parsedConfig,
      });

      navigate(`/roles/${role.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create role");
      setIsSubmitting(false);
    }
  };

  if (loadingOs) {
    return <div className="p-4">Loading...</div>;
  }

  if (operatingSystems.length === 0) {
    return (
      <div className="space-y-4 max-w-2xl">
        <PageHeader
          breadcrumbs={[
            { label: "Roles", href: "/roles" },
            { label: "New Role" }
          ]}
          title="Add Role"
        />
        <Card>
          <CardContent className="pt-6">
            <p className="text-center text-gray-600">
              No operating systems available. Please create an operating system first.
            </p>
            <div className="flex justify-center mt-4">
              <Button onClick={() => navigate('/operating-systems/new')}>
                Create Operating System
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-4 max-w-4xl">
      <PageHeader
        breadcrumbs={[
          { label: "Roles", href: "/roles" },
          { label: "New Role" }
        ]}
        title="Add Role"
        description="Define a provisioning role with OS, disk layout, and configuration"
      />

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Basic Information */}
        <Card>
          <CardHeader>
            <CardTitle>Basic Information</CardTitle>
            <CardDescription>
              Define the role name, description, and operating system
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <FormField
              id="name"
              label="Name"
              required
              value={name}
              onChange={setName}
              placeholder="e.g., web-server"
            />

            <FormTextareaField
              id="description"
              label="Description"
              value={description}
              onChange={setDescription}
              placeholder="Optional description"
              rows={2}
            />

            <FormSelectField
              id="os"
              label="Operating System"
              required
              value={osId || ''}
              onChange={(value) => setOsId(parseInt(value))}
              options={operatingSystems.map((os) => ({
                value: os.id!,
                label: `${os.name} ${os.version}`
              }))}
              helperText="Supported architectures will be inferred from the selected OS"
            />
          </CardContent>
        </Card>

        {/* Disk Layout */}
        <Card>
          <CardHeader>
            <CardTitle>Disk Layout</CardTitle>
            <CardDescription>
              Define the partition scheme for devices with this role
            </CardDescription>
          </CardHeader>
          <CardContent>
            <PartitionEditor partitions={partitions} onChange={setPartitions} />
          </CardContent>
        </Card>

        {/* Config Template */}
        <Card>
          <CardHeader>
            <CardTitle>Configuration Template</CardTitle>
            <CardDescription>
              Optional JSON configuration that will be available in install scripts
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
              helperText="This configuration will be accessible in install scripts via template variables"
            />
          </CardContent>
        </Card>

        {error && (
          <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
            {error}
          </div>
        )}

        <div className="flex gap-2">
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Creating..." : "Create Role"}
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
    </div>
  );
}

export default RoleNew;
