import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Trash2 } from "lucide-react";
import type { DiskConfig, PartitionConfig } from "@/lib/client";
import PartitionRow from "./partition-row";

interface DiskSectionProps {
  disk: DiskConfig;
  diskIndex: number;
  vgNames: string[];
  canRemove: boolean;
  onUpdateLabel: (label: string) => void;
  onUpdateTable: (table: string) => void;
  onRemove: () => void;
  onAddPartition: () => void;
  onRemovePartition: (partIndex: number) => void;
  onUpdatePartition: (partIndex: number, partition: PartitionConfig) => void;
  errors?: Record<string, string>;
  errorPrefix: string;
  onClearError?: (key: string) => void;
}

export default function DiskSection({
  disk,
  diskIndex,
  vgNames,
  canRemove,
  onUpdateLabel,
  onUpdateTable,
  onRemove,
  onAddPartition,
  onRemovePartition,
  onUpdatePartition,
  errors,
  errorPrefix,
  onClearError,
}: DiskSectionProps) {
  const [newPartIndices, setNewPartIndices] = useState<Set<number>>(new Set());

  const deviceErrorKey = `${errorPrefix}.device`;
  const deviceError = errors?.[deviceErrorKey];

  // Determine if any partition has LVM flag for the header badge
  const hasLvm = disk.partitions.some((p) => p.flags?.includes("lvm"));

  function handleAddPartition() {
    setNewPartIndices((prev) => new Set(prev).add(disk.partitions.length));
    onAddPartition();
  }

  return (
    <div className="border border-border mb-3">
      {/* Disk header: accent label + partition type badge + actions */}
      <div className="flex items-center justify-between px-3 py-2 bg-bg-raised border-b border-border">
        <div className="flex items-center gap-3 flex-1 min-w-0">
          {/* Editable disk label */}
          <div className="flex items-center gap-2 flex-1 min-w-0">
            <Label htmlFor={`disk-label-${diskIndex}`} className="sr-only">
              Disk Device Label
            </Label>
            <Input
              id={`disk-label-${diskIndex}`}
              value={disk.device}
              onChange={(e) => {
                onUpdateLabel(e.target.value);
                onClearError?.(deviceErrorKey);
              }}
              placeholder="ROOT, DATA1 or /dev/disk/by-path/..."
              aria-label={`Disk ${diskIndex + 1} device label`}
              aria-invalid={!!deviceError}
              className="h-7 text-xs font-semibold text-accent bg-transparent border-transparent focus:border-border max-w-[200px] px-0 font-mono"
            />
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {/* Partition table toggle */}
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => onUpdateTable("gpt")}
              aria-pressed={disk.partition_table === "gpt"}
              className={`text-xs px-2 py-0.5 rounded-sm border transition-colors ${
                disk.partition_table === "gpt"
                  ? "border-accent text-accent bg-accent-muted"
                  : "border-border text-text-secondary hover:border-border hover:text-text-primary"
              }`}
            >
              GPT
            </button>
            <button
              type="button"
              onClick={() => onUpdateTable("msdos")}
              aria-pressed={disk.partition_table === "msdos"}
              className={`text-xs px-2 py-0.5 rounded-sm border transition-colors ${
                disk.partition_table === "msdos"
                  ? "border-accent text-accent bg-accent-muted"
                  : "border-border text-text-secondary hover:border-border hover:text-text-primary"
              }`}
            >
              MBR
            </button>
          </div>

          {/* LVM indicator badge */}
          {hasLvm && (
            <span className="text-xs text-text-secondary">LVM</span>
          )}

          {/* Remove disk button */}
          <button
            type="button"
            onClick={onRemove}
            disabled={!canRemove}
            aria-label={`Remove disk ${diskIndex + 1}`}
            className="text-text-muted hover:text-status-broken transition-colors disabled:opacity-30 disabled:cursor-not-allowed cursor-pointer"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {deviceError && (
        <div className="px-3 py-1">
          <FormFieldError error={deviceError} />
        </div>
      )}

      {/* Partition table */}
      {disk.partitions.length > 0 && (
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Mount</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Size</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">FS</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Flags</th>
              <th className="px-3 py-1 w-10"></th>
            </tr>
          </thead>
          <tbody>
            {disk.partitions.map((partition, partIndex) => (
              <PartitionRow
                key={partIndex}
                partition={partition}
                partIndex={partIndex}
                vgNames={vgNames}
                onUpdate={(updated) => onUpdatePartition(partIndex, updated)}
                onRemove={() => onRemovePartition(partIndex)}
                errors={errors}
                errorPrefix={`${errorPrefix}.partitions.${partIndex}`}
                onClearError={onClearError}
                initiallyExpanded={newPartIndices.has(partIndex)}
              />
            ))}
          </tbody>
        </table>
      )}

      {disk.partitions.length === 0 && (
        <div className="px-3 py-3 text-xs text-text-muted text-center">
          No partitions defined.
        </div>
      )}

      {/* Add Partition link */}
      <div className="px-3 py-1 border-t border-border-muted">
        <button
          type="button"
          onClick={handleAddPartition}
          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
        >
          + Add Partition
        </button>
      </div>
    </div>
  );
}
