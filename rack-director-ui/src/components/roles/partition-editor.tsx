import { useState, useMemo } from "react";
import type { Partition } from "@/lib/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Plus, Trash2, AlertCircle } from "lucide-react";

interface PartitionEditorProps {
  partitions: Partition[];
  onChange: (partitions: Partition[]) => void;
}

// Common partition flags with descriptions
const PARTITION_FLAGS = [
  { value: "boot", label: "Boot", description: "Bootable flag (MBR)" },
  { value: "esp", label: "ESP", description: "EFI System Partition (UEFI)" },
  { value: "bios_grub", label: "BIOS GRUB", description: "BIOS boot partition (GPT)" },
  { value: "primary", label: "Primary", description: "Primary partition (MBR)" },
  { value: "logical", label: "Logical", description: "Logical partition (MBR)" },
  { value: "extended", label: "Extended", description: "Extended partition (MBR)" },
  { value: "swap", label: "Swap", description: "Swap partition" },
  { value: "lvm", label: "LVM", description: "LVM partition" },
  { value: "raid", label: "RAID", description: "Software RAID partition" },
  { value: "hidden", label: "Hidden", description: "Hidden partition" },
  { value: "root", label: "Root", description: "Root partition" },
  { value: "legacy_boot", label: "Legacy Boot", description: "Legacy boot flag" },
];

// Extract disk name from device path (e.g., /dev/sda1 -> sda)
const getDiskName = (device: string): string => {
  const match = device.match(/\/dev\/([a-z]+)/);
  return match ? match[1] : device;
};

export default function PartitionEditor({ partitions, onChange }: PartitionEditorProps) {
  const [editingIndex, setEditingIndex] = useState<number | null>(null);

  const formatSize = (size: string): string => {
    return size === '*' ? 'Remaining space' : size;
  };

  // Validation: check for flag conflicts across partitions
  const validationErrors = useMemo(() => {
    const errors: { [key: number]: string[] } = {};

    // Group partitions by disk
    const diskPartitions: { [disk: string]: number[] } = {};
    partitions.forEach((partition, index) => {
      const disk = getDiskName(partition.device);
      if (!diskPartitions[disk]) {
        diskPartitions[disk] = [];
      }
      diskPartitions[disk].push(index);
    });

    // Check for multiple exclusive flags per disk
    Object.entries(diskPartitions).forEach(([disk, indices]) => {
      const exclusiveFlags = ['boot', 'esp', 'bios_grub'];

      exclusiveFlags.forEach(flag => {
        const partitionsWithFlag = indices.filter(i => partitions[i].flags.includes(flag));
        if (partitionsWithFlag.length > 1) {
          partitionsWithFlag.forEach(i => {
            if (!errors[i]) errors[i] = [];
            errors[i].push(`Only one partition per disk (${disk}) can have the '${flag}' flag`);
          });
        }
      });
    });

    // Check for conflicting flags on same partition
    partitions.forEach((partition, index) => {
      const flags = partition.flags;

      // Can't have both primary and logical
      if (flags.includes('primary') && flags.includes('logical')) {
        if (!errors[index]) errors[index] = [];
        errors[index].push("Cannot have both 'primary' and 'logical' flags");
      }

      // ESP should be vfat filesystem
      if (flags.includes('esp') && partition.filesystem !== 'vfat') {
        if (!errors[index]) errors[index] = [];
        errors[index].push("ESP partitions should use 'vfat' filesystem");
      }

      // Swap flag should match swap filesystem
      if (flags.includes('swap') && partition.filesystem !== 'swap') {
        if (!errors[index]) errors[index] = [];
        errors[index].push("Swap flag should be used with 'swap' filesystem");
      }
    });

    return errors;
  }, [partitions]);

  const addPartition = () => {
    const newPartition: Partition = {
      device: "/dev/sda" + (partitions.length + 1),
      size: "10G",
      filesystem: "ext4",
      mount_point: "",
      flags: [],
    };
    onChange([...partitions, newPartition]);
    setEditingIndex(partitions.length);
  };

  const updatePartition = (index: number, field: keyof Partition, value: any) => {
    const updated = [...partitions];
    if (field === 'mount_point') {
      updated[index] = {
        ...updated[index],
        mount_point: value || undefined
      };
    } else {
      updated[index] = { ...updated[index], [field]: value };
    }
    onChange(updated);
  };

  const toggleFlag = (index: number, flag: string) => {
    const updated = [...partitions];
    const currentFlags = updated[index].flags;

    if (currentFlags.includes(flag)) {
      updated[index].flags = currentFlags.filter(f => f !== flag);
    } else {
      updated[index].flags = [...currentFlags, flag];
    }

    onChange(updated);
  };

  const deletePartition = (index: number) => {
    const updated = partitions.filter((_, i) => i !== index);
    onChange(updated);
    if (editingIndex === index) {
      setEditingIndex(null);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <Label className="text-base font-semibold">Disk Partitions *</Label>
        <Button type="button" variant="outline" size="sm" onClick={addPartition}>
          <Plus className="h-4 w-4 mr-2" />
          Add Partition
        </Button>
      </div>

      {partitions.length === 0 ? (
        <Card>
          <CardContent className="pt-6">
            <p className="text-center text-gray-400">
              No partitions defined. Click "Add Partition" to get started.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {partitions.map((partition, index) => (
            <Card key={index} className={`border-2 ${validationErrors[index] ? 'border-orange-300' : ''}`}>
              <CardHeader className="pb-3">
                <div className="flex justify-between items-center">
                  <CardTitle className="text-sm font-medium">
                    {partition.device} → {partition.mount_point || '(no mount point)'}
                  </CardTitle>
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => setEditingIndex(editingIndex === index ? null : index)}
                    >
                      {editingIndex === index ? 'Collapse' : 'Edit'}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => deletePartition(index)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
                <CardDescription className="text-xs">
                  {formatSize(partition.size)} • {partition.filesystem}
                  {partition.flags.length > 0 && ` • Flags: ${partition.flags.join(', ')}`}
                </CardDescription>

                {validationErrors[index] && (
                  <div className="mt-2 space-y-1">
                    {validationErrors[index].map((error, i) => (
                      <div key={i} className="flex items-start gap-2 text-xs text-orange-600">
                        <AlertCircle className="h-3 w-3 mt-0.5 flex-shrink-0" />
                        <span>{error}</span>
                      </div>
                    ))}
                  </div>
                )}
              </CardHeader>

              {editingIndex === index && (
                <CardContent className="space-y-4">
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <Label htmlFor={`device-${index}`}>Device *</Label>
                      <Input
                        id={`device-${index}`}
                        value={partition.device}
                        onChange={(e) => updatePartition(index, 'device', e.target.value)}
                        placeholder="/dev/sda1"
                        required
                      />
                      <p className="text-xs text-gray-500">
                        e.g., /dev/sda1, /dev/nvme0n1p1
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`size-${index}`}>Size *</Label>
                      <div className="flex gap-2">
                        <Input
                          id={`size-${index}`}
                          value={partition.size}
                          onChange={(e) => updatePartition(index, 'size', e.target.value)}
                          placeholder="100G or *"
                          required
                          className="flex-1"
                        />
                        <Button
                          type="button"
                          variant={partition.size === '*' ? 'default' : 'outline'}
                          size="sm"
                          onClick={() => updatePartition(index, 'size', partition.size === '*' ? '10G' : '*')}
                          className="whitespace-nowrap"
                        >
                          {partition.size === '*' ? 'Set Size' : 'Use *'}
                        </Button>
                      </div>
                      <p className="text-xs text-gray-500">
                        Use * for "remaining space"
                      </p>
                    </div>
                  </div>

                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <Label htmlFor={`filesystem-${index}`}>Filesystem *</Label>
                      <select
                        id={`filesystem-${index}`}
                        value={partition.filesystem}
                        onChange={(e) => updatePartition(index, 'filesystem', e.target.value)}
                        className="w-full border rounded-md px-3 py-2"
                        required
                      >
                        <option value="ext4">ext4</option>
                        <option value="ext3">ext3</option>
                        <option value="xfs">xfs</option>
                        <option value="btrfs">btrfs</option>
                        <option value="swap">swap</option>
                        <option value="vfat">vfat (FAT32)</option>
                      </select>
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`mount-${index}`}>Mount Point</Label>
                      <Input
                        id={`mount-${index}`}
                        value={partition.mount_point || ''}
                        onChange={(e) => updatePartition(index, 'mount_point', e.target.value)}
                        placeholder="/ or /var or /home"
                      />
                      <p className="text-xs text-gray-500">
                        Leave empty for swap
                      </p>
                    </div>
                  </div>

                  <div className="space-y-2">
                    <Label>Partition Flags</Label>
                    <div className="grid grid-cols-2 gap-2 p-3 border rounded-md bg-gray-50">
                      {PARTITION_FLAGS.map(flag => (
                        <label
                          key={flag.value}
                          className="flex items-start gap-2 cursor-pointer hover:bg-gray-100 p-2 rounded"
                        >
                          <input
                            type="checkbox"
                            checked={partition.flags.includes(flag.value)}
                            onChange={() => toggleFlag(index, flag.value)}
                            className="mt-1"
                          />
                          <div className="flex-1">
                            <div className="font-medium text-sm">{flag.label}</div>
                            <div className="text-xs text-gray-600">{flag.description}</div>
                          </div>
                        </label>
                      ))}
                    </div>
                    <p className="text-xs text-gray-500">
                      Select appropriate flags for this partition
                    </p>
                  </div>

                  {partition.flags.length > 0 && (
                    <div className="flex flex-wrap gap-2 p-2 bg-blue-50 rounded">
                      <span className="text-xs font-medium text-gray-600">Selected:</span>
                      {partition.flags.map(flag => (
                        <Badge key={flag} variant="secondary" className="text-xs">
                          {flag}
                        </Badge>
                      ))}
                    </div>
                  )}
                </CardContent>
              )}
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
