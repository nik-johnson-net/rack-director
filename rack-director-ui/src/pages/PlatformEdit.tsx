import { useState } from "react";
import { useNavigate, useLoaderData } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { FormField, FormTextareaField, FormSelectField } from "@/components/ui/form-field";
import { updatePlatform, type Platform, type PlatformDisk, type PlatformNic, type PlatformCpu, type DiskType } from "@/lib/client";
import { Plus, Trash2 } from "lucide-react";
import { useFieldErrors } from "@/hooks/useFieldErrors";

const DISK_TYPES: DiskType[] = ["nvme", "ssd", "hdd"];

function PlatformEdit() {
  const navigate = useNavigate();
  const platform = useLoaderData<Platform>();
  const { clearFieldError, getError, setErrors } = useFieldErrors();

  const [name, setName] = useState(platform.name);
  const [description, setDescription] = useState(platform.description || "");
  const [memoryGib, setMemoryGib] = useState(platform.attributes.memory_gib);

  // Disks state
  const [disks, setDisks] = useState<PlatformDisk[]>(platform.attributes.disks);
  const [diskPath, setDiskPath] = useState("");
  const [diskSizeGb, setDiskSizeGb] = useState<number>(0);
  const [diskType, setDiskType] = useState<DiskType>("nvme");
  const [diskLabel, setDiskLabel] = useState("");

  // NICs state
  const [nics, setNics] = useState<PlatformNic[]>(platform.attributes.nics);
  const [nicLogical, setNicLogical] = useState("");
  const [nicSpeedGbps, setNicSpeedGbps] = useState<number>(0);
  const [nicLabel, setNicLabel] = useState("");

  // CPUs state
  const [cpus, setCpus] = useState<PlatformCpu[]>(platform.attributes.cpus);
  const [cpuBrand, setCpuBrand] = useState("");
  const [cpuModel, setCpuModel] = useState("");
  const [cpuCores, setCpuCores] = useState<number>(0);

  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleAddDisk = () => {
    if (!diskPath || diskSizeGb <= 0) {
      setError("Please provide valid disk path and size");
      return;
    }

    // Check for duplicate labels
    if (diskLabel && disks.some(d => d.label === diskLabel)) {
      setError(`Disk label "${diskLabel}" is already used`);
      return;
    }

    setDisks([...disks, {
      path: diskPath,
      size_gb: diskSizeGb,
      disk_type: diskType,
      label: diskLabel || undefined
    }]);

    // Reset form
    setDiskPath("");
    setDiskSizeGb(0);
    setDiskLabel("");
    setError(null);
  };

  const handleRemoveDisk = (index: number) => {
    setDisks(disks.filter((_, i) => i !== index));
  };

  const handleAddNic = () => {
    if (!nicLogical) {
      setError("Please provide NIC logical name");
      return;
    }

    // Check for duplicate labels
    if (nicLabel && nics.some(n => n.label === nicLabel)) {
      setError(`NIC label "${nicLabel}" is already used`);
      return;
    }

    setNics([...nics, {
      logical: nicLogical,
      speed_gbps: nicSpeedGbps > 0 ? nicSpeedGbps : undefined,
      label: nicLabel || undefined
    }]);

    // Reset form
    setNicLogical("");
    setNicSpeedGbps(0);
    setNicLabel("");
    setError(null);
  };

  const handleRemoveNic = (index: number) => {
    setNics(nics.filter((_, i) => i !== index));
  };

  const handleAddCpu = () => {
    if (!cpuBrand || !cpuModel || cpuCores <= 0) {
      setError("Please provide valid CPU brand, model, and cores");
      return;
    }

    setCpus([...cpus, {
      brand: cpuBrand,
      model: cpuModel,
      cores: cpuCores
    }]);

    // Reset form
    setCpuBrand("");
    setCpuModel("");
    setCpuCores(0);
    setError(null);
  };

  const handleRemoveCpu = (index: number) => {
    setCpus(cpus.filter((_, i) => i !== index));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setErrors({});

    if (!name.trim()) {
      setError("Platform name is required");
      return;
    }

    if (disks.length === 0) {
      setError("At least one disk is required");
      return;
    }

    if (nics.length === 0) {
      setError("At least one NIC is required");
      return;
    }

    if (cpus.length === 0) {
      setError("At least one CPU is required");
      return;
    }

    if (memoryGib <= 0) {
      setError("Memory must be greater than 0");
      return;
    }

    setIsSubmitting(true);

    try {
      await updatePlatform(platform.id!, {
        name,
        description: description || undefined,
        attributes: {
          disks,
          nics,
          cpus,
          memory_gib: memoryGib
        }
      });

      navigate('/platforms');
    } catch (err) {
      if (err instanceof Error) {
        setError(err.message);
      } else {
        setError("Failed to update platform");
      }
      setIsSubmitting(false);
    }
  };

  return (
    <div className="space-y-4 max-w-4xl">
      <PageHeader
        breadcrumbs={[
          { label: "Platforms", href: "/platforms" },
          { label: platform.name }
        ]}
        title="Edit Platform"
        description="Update the hardware platform configuration"
      />

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Warning about editing platforms */}
        <div className="rounded-md bg-yellow-50 dark:bg-yellow-950/20 border border-yellow-200 dark:border-yellow-900 p-4">
          <div className="flex">
            <div className="ml-3">
              <h3 className="text-sm font-medium text-yellow-800 dark:text-yellow-200">
                Warning: Editing Platforms
              </h3>
              <div className="mt-2 text-sm text-yellow-700 dark:text-yellow-300">
                <p>
                  Editing platforms after devices have been assigned may cause configuration
                  inconsistencies. Platform edits should only be made if you understand the
                  impact on assigned devices.
                </p>
              </div>
            </div>
          </div>
        </div>

        {/* Basic Information */}
        <Card>
          <CardHeader>
            <CardTitle>Basic Information</CardTitle>
            <CardDescription>
              Define the platform name and description
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <FormField
              id="name"
              label="Name"
              required
              value={name}
              onChange={setName}
              placeholder="e.g., Dell PowerEdge R640"
              error={getError("name")}
              onClearError={() => clearFieldError("name")}
            />

            <FormTextareaField
              id="description"
              label="Description"
              value={description}
              onChange={setDescription}
              placeholder="Optional description"
              rows={2}
            />
          </CardContent>
        </Card>

        {/* Disks */}
        <Card>
          <CardHeader>
            <CardTitle>Disks</CardTitle>
            <CardDescription>
              Define the disk configuration for this platform
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormField
                id="diskPath"
                label="Disk Path"
                value={diskPath}
                onChange={setDiskPath}
                placeholder="e.g., /dev/disk/by-path/pci-0000:00:1f.2-ata-1"
              />
              <FormField
                id="diskSizeGb"
                label="Size (GB)"
                type="number"
                value={diskSizeGb > 0 ? String(diskSizeGb) : ''}
                onChange={(val) => setDiskSizeGb(parseInt(val) || 0)}
                placeholder="e.g., 500"
              />
              <FormSelectField
                id="diskType"
                label="Disk Type"
                value={diskType}
                onChange={(val) => setDiskType(val as DiskType)}
                options={DISK_TYPES.map(type => ({ value: type, label: type.toUpperCase() }))}
              />
              <FormField
                id="diskLabel"
                label="Label (optional)"
                value={diskLabel}
                onChange={setDiskLabel}
                placeholder="e.g., ROOT, DATA1"
                helperText="Unique label for this disk"
              />
            </div>
            <Button type="button" onClick={handleAddDisk} variant="outline" size="sm">
              <Plus className="h-4 w-4 mr-2" />
              Add Disk
            </Button>

            {disks.length > 0 && (
              <div className="space-y-2">
                <h4 className="text-sm font-medium">Configured Disks</h4>
                {disks.map((disk, index) => (
                  <div key={index} className="flex items-center justify-between p-2 border rounded">
                    <span className="text-sm font-mono">
                      {disk.path} ({disk.size_gb}GB, {disk.disk_type.toUpperCase()})
                      {disk.label && ` - ${disk.label}`}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => handleRemoveDisk(index)}
                      aria-label="Remove disk"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* NICs */}
        <Card>
          <CardHeader>
            <CardTitle>Network Interfaces</CardTitle>
            <CardDescription>
              Define the NIC configuration for this platform
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormField
                id="nicLogical"
                label="Logical Name"
                value={nicLogical}
                onChange={setNicLogical}
                placeholder="e.g., eno1"
              />
              <FormField
                id="nicSpeedGbps"
                label="Speed (Gbps, optional)"
                type="number"
                value={nicSpeedGbps > 0 ? String(nicSpeedGbps) : ''}
                onChange={(val) => setNicSpeedGbps(parseFloat(val) || 0)}
                placeholder="e.g., 10"
              />
              <FormField
                id="nicLabel"
                label="Label (optional)"
                value={nicLabel}
                onChange={setNicLabel}
                placeholder="e.g., NIC1, NIC2"
                helperText="Unique label for this NIC"
              />
            </div>
            <Button type="button" onClick={handleAddNic} variant="outline" size="sm">
              <Plus className="h-4 w-4 mr-2" />
              Add NIC
            </Button>

            {nics.length > 0 && (
              <div className="space-y-2">
                <h4 className="text-sm font-medium">Configured NICs</h4>
                {nics.map((nic, index) => (
                  <div key={index} className="flex items-center justify-between p-2 border rounded">
                    <span className="text-sm font-mono">
                      {nic.logical}
                      {nic.speed_gbps && ` (${nic.speed_gbps} Gbps)`}
                      {nic.label && ` - ${nic.label}`}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => handleRemoveNic(index)}
                      aria-label="Remove NIC"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* CPUs */}
        <Card>
          <CardHeader>
            <CardTitle>CPUs</CardTitle>
            <CardDescription>
              Define the CPU configuration for this platform
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormField
                id="cpuBrand"
                label="Brand"
                value={cpuBrand}
                onChange={setCpuBrand}
                placeholder="e.g., intel, amd"
              />
              <FormField
                id="cpuModel"
                label="Model"
                value={cpuModel}
                onChange={setCpuModel}
                placeholder="e.g., E3-1240 v3"
              />
              <FormField
                id="cpuCores"
                label="Cores"
                type="number"
                value={cpuCores > 0 ? String(cpuCores) : ''}
                onChange={(val) => setCpuCores(parseInt(val) || 0)}
                placeholder="e.g., 12"
              />
            </div>
            <Button type="button" onClick={handleAddCpu} variant="outline" size="sm">
              <Plus className="h-4 w-4 mr-2" />
              Add CPU
            </Button>

            {cpus.length > 0 && (
              <div className="space-y-2">
                <h4 className="text-sm font-medium">Configured CPUs</h4>
                {cpus.map((cpu, index) => (
                  <div key={index} className="flex items-center justify-between p-2 border rounded">
                    <span className="text-sm">
                      {cpu.brand} {cpu.model} ({cpu.cores} cores)
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => handleRemoveCpu(index)}
                      aria-label="Remove CPU"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Memory */}
        <Card>
          <CardHeader>
            <CardTitle>Memory</CardTitle>
            <CardDescription>
              Define the memory configuration for this platform
            </CardDescription>
          </CardHeader>
          <CardContent>
            <FormField
              id="memoryGib"
              label="Memory (GiB)"
              type="number"
              required
              value={memoryGib > 0 ? String(memoryGib) : ''}
              onChange={(val) => setMemoryGib(parseInt(val) || 0)}
              placeholder="e.g., 64"
            />
          </CardContent>
        </Card>

        {error && (
          <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
            {error}
          </div>
        )}

        <div className="flex gap-2">
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Saving..." : "Save Changes"}
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => navigate('/platforms')}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  );
}

export default PlatformEdit;
