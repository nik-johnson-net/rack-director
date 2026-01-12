import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
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
            <div className="space-y-2">
              <Label htmlFor="name">Name *</Label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g., web-server"
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="description">Description</Label>
              <Textarea
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Optional description"
                rows={2}
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="os">Operating System *</Label>
              <select
                id="os"
                value={osId || ''}
                onChange={(e) => setOsId(parseInt(e.target.value))}
                className="w-full border rounded-md px-3 py-2"
                required
              >
                {operatingSystems.map((os) => (
                  <option key={os.id} value={os.id}>
                    {os.name} {os.version}
                  </option>
                ))}
              </select>
              <p className="text-xs text-gray-500">
                Supported architectures will be inferred from the selected OS
              </p>
            </div>
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
          <CardContent className="space-y-2">
            <Label htmlFor="config">JSON Configuration</Label>
            <Textarea
              id="config"
              value={configTemplate}
              onChange={(e) => setConfigTemplate(e.target.value)}
              placeholder={'{\n  "packages": ["nginx", "postgresql"],\n  "custom_setting": "value"\n}'}
              rows={8}
              className="font-mono text-sm"
            />
            <p className="text-xs text-gray-500">
              This configuration will be accessible in install scripts via template variables
            </p>
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
