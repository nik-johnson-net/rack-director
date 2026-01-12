import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import PartitionEditor from "@/components/roles/partition-editor";
import {
  updateRole,
  deleteRole,
  getRoleDevices,
  getOperatingSystems,
  type RoleWithOs,
  type Partition,
  type OperatingSystem,
} from "@/lib/client";
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
  const [partitions, setPartitions] = useState<Partition[]>(data.disk_layout.partitions);
  const [configTemplate, setConfigTemplate] = useState(
    data.config_template ? JSON.stringify(data.config_template, null, 2) : ""
  );
  const [operatingSystems, setOperatingSystems] = useState<OperatingSystem[]>([]);
  const [assignedDevices, setAssignedDevices] = useState<string[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

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
      const updated = await updateRole(roleId, {
        name,
        description: description || undefined,
        os_id: osId,
        disk_layout: { partitions },
        config_template: parsedConfig,
      });

      // Update local state with flattened data
      setData({ ...data, ...updated, os_name: data.os_name, os_version: data.os_version });
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update role");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteRole(roleId);
      navigate('/roles');
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete role");
    }
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
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="destructive">
                <Trash2 className="h-4 w-4 mr-2" />
                Delete
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Are you sure?</AlertDialogTitle>
                <AlertDialogDescription>
                  This will permanently delete this role. This action cannot be undone.
                  {assignedDevices.length > 0 && (
                    <span className="block mt-2 font-semibold text-orange-600">
                      Warning: This role is assigned to {assignedDevices.length} device(s).
                    </span>
                  )}
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction onClick={handleDelete}>Delete</AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        }
      />

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}

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
            <div className="space-y-2">
              <Label htmlFor="name">Name *</Label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="description">Description</Label>
              <Textarea
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={2}
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="os">Operating System *</Label>
              <select
                id="os"
                value={osId}
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
                Supported architectures are inferred from the selected OS
              </p>
            </div>
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
            <PartitionEditor partitions={partitions} onChange={setPartitions} />
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
              This configuration is accessible in install scripts via template variables
            </p>
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
    </div>
  );
}

export default RoleEdit;
