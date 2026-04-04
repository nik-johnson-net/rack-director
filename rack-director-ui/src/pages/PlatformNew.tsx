import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PageHeader } from "@/components/ui/page-header";
import { createPlatform, type PlatformDisk, type PlatformNic, type PlatformCpu, type DiskType } from "@/lib/client";
import { Plus, Trash2 } from "lucide-react";

const DISK_TYPES: DiskType[] = ["nvme", "ssd", "hdd"];

const labelClass = "text-xs text-text-secondary uppercase tracking-[0.5px]";
const inputClass = "h-8 text-xs bg-bg-base border-border text-text-primary";
const selectClass =
  "h-8 w-full bg-bg-base border border-border text-text-primary text-xs px-2 py-1 focus:outline-none focus:border-accent appearance-none cursor-pointer";

function SectionCard({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border bg-bg-surface">
      <div className="px-4 py-3 border-b border-border">
        <span className="text-sm font-semibold text-text-primary">{title}</span>
        {subtitle && (
          <p className="text-xs text-text-secondary mt-0.5">{subtitle}</p>
        )}
      </div>
      <div className="px-4 py-4 space-y-4">{children}</div>
    </div>
  );
}

function PlatformNew() {
  const navigate = useNavigate();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [memoryGib, setMemoryGib] = useState<number>(0);

  // Disks state
  const [disks, setDisks] = useState<PlatformDisk[]>([]);
  const [diskPath, setDiskPath] = useState("");
  const [diskSizeGb, setDiskSizeGb] = useState<number>(0);
  const [diskType, setDiskType] = useState<DiskType>("nvme");
  const [diskLabel, setDiskLabel] = useState("");

  // NICs state
  const [nics, setNics] = useState<PlatformNic[]>([]);
  const [nicLogical, setNicLogical] = useState("");
  const [nicSpeedGbps, setNicSpeedGbps] = useState<number>(0);
  const [nicLabel, setNicLabel] = useState("");

  // CPUs state
  const [cpus, setCpus] = useState<PlatformCpu[]>([]);
  const [cpuBrand, setCpuBrand] = useState("");
  const [cpuModel, setCpuModel] = useState("");
  const [cpuCores, setCpuCores] = useState<number>(0);

  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleAddDisk = () => {
    if (!diskPath || diskSizeGb <= 0) {
      setError("Please provide a valid disk path and size.");
      return;
    }
    if (diskLabel && disks.some((d) => d.label === diskLabel)) {
      setError(`Disk label "${diskLabel}" is already used.`);
      return;
    }
    setDisks([
      ...disks,
      { path: diskPath, size_gb: diskSizeGb, disk_type: diskType, label: diskLabel || undefined },
    ]);
    setDiskPath("");
    setDiskSizeGb(0);
    setDiskLabel("");
    setError(null);
  };

  const handleAddNic = () => {
    if (!nicLogical) {
      setError("Please provide a NIC logical name.");
      return;
    }
    if (nicLabel && nics.some((n) => n.label === nicLabel)) {
      setError(`NIC label "${nicLabel}" is already used.`);
      return;
    }
    setNics([
      ...nics,
      { logical: nicLogical, speed_gbps: nicSpeedGbps > 0 ? nicSpeedGbps : undefined, label: nicLabel || undefined },
    ]);
    setNicLogical("");
    setNicSpeedGbps(0);
    setNicLabel("");
    setError(null);
  };

  const handleAddCpu = () => {
    if (!cpuBrand || !cpuModel || cpuCores <= 0) {
      setError("Please provide a valid CPU brand, model, and core count.");
      return;
    }
    setCpus([...cpus, { brand: cpuBrand, model: cpuModel, cores: cpuCores }]);
    setCpuBrand("");
    setCpuModel("");
    setCpuCores(0);
    setError(null);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!name.trim()) { setError("Platform name is required."); return; }
    if (disks.length === 0) { setError("At least one disk is required."); return; }
    if (nics.length === 0) { setError("At least one NIC is required."); return; }
    if (cpus.length === 0) { setError("At least one CPU is required."); return; }
    if (memoryGib <= 0) { setError("Memory must be greater than 0."); return; }

    setIsSubmitting(true);
    try {
      const platform = await createPlatform({
        name,
        description: description || undefined,
        attributes: { disks, nics, cpus, memory_gib: memoryGib },
      });
      navigate(`/platforms/${platform.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create platform");
      setIsSubmitting(false);
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Platforms", href: "/platforms" },
          { label: "New Platform" },
        ]}
        title="New Platform"
        description="Define a hardware platform with disks, NICs, CPUs, and memory"
      />

      <form onSubmit={handleSubmit}>
        <div style={{ maxWidth: 700 }} className="space-y-4">
          {/* General */}
          <SectionCard title="General">
            <div className="space-y-1">
              <Label htmlFor="name" className={labelClass}>Name *</Label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g., Dell PowerEdge R640"
                className={inputClass}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="description" className={labelClass}>Description</Label>
              <Input
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Optional description"
                className={inputClass}
              />
            </div>
          </SectionCard>

          {/* Disks */}
          <SectionCard title="Disks" subtitle="Define the disk configuration for this platform">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="space-y-1">
                <Label htmlFor="diskPath" className={labelClass}>Disk Path</Label>
                <Input
                  id="diskPath"
                  value={diskPath}
                  onChange={(e) => setDiskPath(e.target.value)}
                  placeholder="e.g., /dev/disk/by-path/..."
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="diskSizeGb" className={labelClass}>Size (GB)</Label>
                <Input
                  id="diskSizeGb"
                  type="number"
                  value={diskSizeGb > 0 ? String(diskSizeGb) : ""}
                  onChange={(e) => setDiskSizeGb(parseInt(e.target.value) || 0)}
                  placeholder="e.g., 500"
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="diskType" className={labelClass}>Disk Type</Label>
                <div className="relative">
                  <select
                    id="diskType"
                    value={diskType}
                    onChange={(e) => setDiskType(e.target.value as DiskType)}
                    className={selectClass}
                  >
                    {DISK_TYPES.map((t) => (
                      <option key={t} value={t}>{t.toUpperCase()}</option>
                    ))}
                  </select>
                  <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-text-muted">
                    <svg width="10" height="6" viewBox="0 0 10 6" fill="currentColor">
                      <path d="M0 0l5 6 5-6z" />
                    </svg>
                  </span>
                </div>
              </div>
              <div className="space-y-1">
                <Label htmlFor="diskLabel" className={labelClass}>Label (optional)</Label>
                <Input
                  id="diskLabel"
                  value={diskLabel}
                  onChange={(e) => setDiskLabel(e.target.value)}
                  placeholder="e.g., ROOT, DATA1"
                  className={inputClass}
                />
              </div>
            </div>
            <button
              type="button"
              onClick={handleAddDisk}
              className="flex items-center gap-1.5 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
            >
              <Plus className="h-3 w-3" />
              Add Disk
            </button>
            {disks.length > 0 && (
              <div className="border border-border">
                <table className="w-full border-collapse">
                  <thead>
                    <tr className="bg-bg-raised">
                      {["Path", "Size", "Type", "Label", ""].map((col, i) => (
                        <th key={i} className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border">
                          {col}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {disks.map((disk, index) => (
                      <tr key={index} className="border-b border-border-muted last:border-b-0 bg-bg-base">
                        <td className="px-3 py-2 text-xs font-mono text-text-secondary">{disk.path}</td>
                        <td className="px-3 py-2 text-xs text-text-primary">{disk.size_gb} GB</td>
                        <td className="px-3 py-2 text-xs text-text-secondary uppercase">{disk.disk_type}</td>
                        <td className="px-3 py-2 text-xs text-accent font-semibold">{disk.label || "—"}</td>
                        <td className="px-3 py-2">
                          <button
                            type="button"
                            onClick={() => setDisks(disks.filter((_, i) => i !== index))}
                            aria-label="Remove disk"
                            className="text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                          >
                            <Trash2 className="h-3 w-3" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </SectionCard>

          {/* NICs */}
          <SectionCard title="Network Interfaces" subtitle="Define the NIC configuration for this platform">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="space-y-1">
                <Label htmlFor="nicLogical" className={labelClass}>Logical Name</Label>
                <Input
                  id="nicLogical"
                  value={nicLogical}
                  onChange={(e) => setNicLogical(e.target.value)}
                  placeholder="e.g., eno1"
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="nicSpeed" className={labelClass}>Speed (Gbps, optional)</Label>
                <Input
                  id="nicSpeed"
                  type="number"
                  value={nicSpeedGbps > 0 ? String(nicSpeedGbps) : ""}
                  onChange={(e) => setNicSpeedGbps(parseFloat(e.target.value) || 0)}
                  placeholder="e.g., 10"
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="nicLabel" className={labelClass}>Label (optional)</Label>
                <Input
                  id="nicLabel"
                  value={nicLabel}
                  onChange={(e) => setNicLabel(e.target.value)}
                  placeholder="e.g., NIC1, NIC2"
                  className={inputClass}
                />
              </div>
            </div>
            <button
              type="button"
              onClick={handleAddNic}
              className="flex items-center gap-1.5 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
            >
              <Plus className="h-3 w-3" />
              Add NIC
            </button>
            {nics.length > 0 && (
              <div className="border border-border">
                <table className="w-full border-collapse">
                  <thead>
                    <tr className="bg-bg-raised">
                      {["Logical", "Speed", "Label", ""].map((col, i) => (
                        <th key={i} className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border">
                          {col}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {nics.map((nic, index) => (
                      <tr key={index} className="border-b border-border-muted last:border-b-0 bg-bg-base">
                        <td className="px-3 py-2 text-xs font-mono text-text-secondary">{nic.logical}</td>
                        <td className="px-3 py-2 text-xs text-text-primary">{nic.speed_gbps != null ? `${nic.speed_gbps} Gbps` : "—"}</td>
                        <td className="px-3 py-2 text-xs text-accent font-semibold">{nic.label || "—"}</td>
                        <td className="px-3 py-2">
                          <button
                            type="button"
                            onClick={() => setNics(nics.filter((_, i) => i !== index))}
                            aria-label="Remove NIC"
                            className="text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                          >
                            <Trash2 className="h-3 w-3" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </SectionCard>

          {/* CPUs */}
          <SectionCard title="CPUs" subtitle="Define the CPU configuration for this platform">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="space-y-1">
                <Label htmlFor="cpuBrand" className={labelClass}>Brand</Label>
                <Input
                  id="cpuBrand"
                  value={cpuBrand}
                  onChange={(e) => setCpuBrand(e.target.value)}
                  placeholder="e.g., intel, amd"
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="cpuModel" className={labelClass}>Model</Label>
                <Input
                  id="cpuModel"
                  value={cpuModel}
                  onChange={(e) => setCpuModel(e.target.value)}
                  placeholder="e.g., E3-1240 v3"
                  className={inputClass}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="cpuCores" className={labelClass}>Cores</Label>
                <Input
                  id="cpuCores"
                  type="number"
                  value={cpuCores > 0 ? String(cpuCores) : ""}
                  onChange={(e) => setCpuCores(parseInt(e.target.value) || 0)}
                  placeholder="e.g., 12"
                  className={inputClass}
                />
              </div>
            </div>
            <button
              type="button"
              onClick={handleAddCpu}
              className="flex items-center gap-1.5 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
            >
              <Plus className="h-3 w-3" />
              Add CPU
            </button>
            {cpus.length > 0 && (
              <div className="border border-border">
                <table className="w-full border-collapse">
                  <thead>
                    <tr className="bg-bg-raised">
                      {["Brand", "Model", "Cores", ""].map((col, i) => (
                        <th key={i} className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border">
                          {col}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {cpus.map((cpu, index) => (
                      <tr key={index} className="border-b border-border-muted last:border-b-0 bg-bg-base">
                        <td className="px-3 py-2 text-xs text-text-primary">{cpu.brand}</td>
                        <td className="px-3 py-2 text-xs text-text-primary">{cpu.model}</td>
                        <td className="px-3 py-2 text-xs text-text-secondary">{cpu.cores}</td>
                        <td className="px-3 py-2">
                          <button
                            type="button"
                            onClick={() => setCpus(cpus.filter((_, i) => i !== index))}
                            aria-label="Remove CPU"
                            className="text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                          >
                            <Trash2 className="h-3 w-3" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </SectionCard>

          {/* Memory */}
          <SectionCard title="Memory">
            <div className="space-y-1" style={{ maxWidth: 200 }}>
              <Label htmlFor="memoryGib" className={labelClass}>Memory (GiB) *</Label>
              <Input
                id="memoryGib"
                type="number"
                value={memoryGib > 0 ? String(memoryGib) : ""}
                onChange={(e) => setMemoryGib(parseInt(e.target.value) || 0)}
                placeholder="e.g., 64"
                className={inputClass}
              />
            </div>
          </SectionCard>

          {error && (
            <div className="px-3 py-2 border border-error-border bg-error-bg text-status-broken text-xs">
              {error}
            </div>
          )}

          <div className="flex gap-2 pb-8">
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? "Creating..." : "Create Platform"}
            </Button>
            <Button
              type="button"
              variant="secondary"
              onClick={() => navigate("/platforms")}
            >
              Cancel
            </Button>
          </div>
        </div>
      </form>
    </div>
  );
}

export default PlatformNew;
