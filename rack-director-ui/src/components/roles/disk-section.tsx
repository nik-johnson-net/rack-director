import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Plus, Trash2 } from "lucide-react";
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

  function handleAddPartition() {
    setNewPartIndices((prev) => new Set(prev).add(disk.partitions.length));
    onAddPartition();
  }

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex flex-col sm:flex-row sm:items-center gap-3">
          {/* Disk badge + label */}
          <div className="flex items-center gap-3 flex-1">
            <Badge variant="secondary" className="shrink-0">
              Disk {diskIndex + 1}
            </Badge>
            <div className="flex-1 space-y-1">
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
                placeholder="ROOT, DATA1, DATA2 or /dev/disk/by-path/..."
                aria-label={`Disk ${diskIndex + 1} device label`}
                aria-invalid={!!deviceError}
                className="h-8 text-sm"
              />
              {deviceError && <FormFieldError error={deviceError} />}
            </div>
          </div>

          {/* Partition table toggle */}
          <div className="flex items-center gap-1 shrink-0">
            <span className="text-xs text-muted-foreground mr-1">Table:</span>
            <Button
              type="button"
              size="sm"
              variant={disk.partition_table === "gpt" ? "default" : "outline"}
              onClick={() => onUpdateTable("gpt")}
              className="h-7 px-2 text-xs"
              aria-pressed={disk.partition_table === "gpt"}
            >
              GPT
            </Button>
            <Button
              type="button"
              size="sm"
              variant={disk.partition_table === "msdos" ? "default" : "outline"}
              onClick={() => onUpdateTable("msdos")}
              className="h-7 px-2 text-xs"
              aria-pressed={disk.partition_table === "msdos"}
            >
              MBR
            </Button>
          </div>

          {/* Remove disk button */}
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onRemove}
            disabled={!canRemove}
            aria-label={`Remove disk ${diskIndex + 1}`}
            className="h-7 w-7 shrink-0"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </CardHeader>

      <CardContent className="space-y-3 pt-0">
        {/* Partition list */}
        {disk.partitions.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-4">
            No partitions defined for this disk.
          </p>
        ) : (
          <div className="space-y-2">
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
          </div>
        )}

        {/* Add Partition button */}
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleAddPartition}
          className="w-full"
        >
          <Plus className="h-4 w-4 mr-2" />
          Add Partition
        </Button>
      </CardContent>
    </Card>
  );
}
