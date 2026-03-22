import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Badge } from "@/components/ui/badge";
import { ChevronDown, ChevronUp, Trash2 } from "lucide-react";
import type { PartitionConfig } from "@/lib/client";
import { selectClassName, BASE_FILESYSTEM_OPTIONS } from "./styles";

interface PartitionRowProps {
  partition: PartitionConfig;
  partIndex: number;
  vgNames: string[];
  onUpdate: (partition: PartitionConfig) => void;
  onRemove: () => void;
  errors?: Record<string, string>;
  errorPrefix: string;
  onClearError?: (key: string) => void;
  initiallyExpanded?: boolean;
}

const FILESYSTEM_OPTIONS = [
  ...BASE_FILESYSTEM_OPTIONS,
  { value: "__lvm__", label: "— LVM Physical Volume" },
];

const ADDITIONAL_FLAGS = ["boot", "esp", "bios_grub"];

function getFilesystemSelectValue(partition: PartitionConfig): string {
  if (partition.flags?.includes("lvm")) return "__lvm__";
  return partition.filesystem ?? "ext4";
}

function getFilesystemSummary(partition: PartitionConfig): string {
  if (partition.flags?.includes("lvm")) return "LVM PV";
  return partition.filesystem ?? "—";
}

export default function PartitionRow({
  partition,
  partIndex,
  vgNames,
  onUpdate,
  onRemove,
  errors,
  errorPrefix,
  onClearError,
  initiallyExpanded = false,
}: PartitionRowProps) {
  const [expanded, setExpanded] = useState(initiallyExpanded);

  const labelError = errors?.[`${errorPrefix}.label`];
  const sizeError = errors?.[`${errorPrefix}.size`];
  const filesystemError = errors?.[`${errorPrefix}.filesystem`];
  const mountPointError = errors?.[`${errorPrefix}.mount_point`];
  const flagsError = errors?.[`${errorPrefix}.flags`];
  const volumeGroupError = errors?.[`${errorPrefix}.volume_group`];

  const isLvm = partition.flags?.includes("lvm") ?? false;
  const showMountPoint =
    !isLvm && partition.filesystem !== "swap";

  function handleFilesystemChange(val: string) {
    onClearError?.(`${errorPrefix}.filesystem`);
    if (val === "__lvm__") {
      // Add lvm flag, clear filesystem and mount_point
      const currentFlags = partition.flags ?? [];
      const newFlags = currentFlags.includes("lvm")
        ? currentFlags
        : [...currentFlags, "lvm"];
      onUpdate({
        ...partition,
        filesystem: undefined,
        mount_point: undefined,
        flags: newFlags,
      });
    } else {
      // Remove lvm from flags
      const newFlags = (partition.flags ?? []).filter((f) => f !== "lvm");
      onUpdate({
        ...partition,
        filesystem: val,
        volume_group: undefined,
        flags: newFlags.length > 0 ? newFlags : undefined,
      });
    }
  }

  function handleFlagToggle(flag: string) {
    onClearError?.(`${errorPrefix}.flags`);
    const current = partition.flags ?? [];
    const newFlags = current.includes(flag)
      ? current.filter((f) => f !== flag)
      : [...current, flag];
    onUpdate({ ...partition, flags: newFlags.length > 0 ? newFlags : undefined });
  }

  const sizeDisplay = partition.size
    ? partition.size === "*" || partition.size === "rest"
      ? "rest"
      : partition.size
    : "—";

  return (
    <div className="border rounded-md bg-card">
      {/* Collapsed summary row */}
      <div className="flex items-center gap-2 px-3 py-2">
        <div className="flex-1 flex flex-wrap items-center gap-2 min-w-0">
          <span className="font-mono text-sm font-medium truncate">
            {partition.label || <span className="text-muted-foreground italic">unnamed</span>}
          </span>
          <Badge variant="outline" className="text-xs shrink-0">
            {sizeDisplay}
          </Badge>
          <span className="text-xs text-muted-foreground shrink-0">
            {getFilesystemSummary(partition)}
          </span>
          {partition.mount_point && (
            <span className="text-xs text-muted-foreground font-mono shrink-0">
              {partition.mount_point}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => setExpanded((e) => !e)}
            aria-label={expanded ? "Collapse partition" : "Expand partition"}
            className="h-7 w-7"
          >
            {expanded ? (
              <ChevronUp className="h-3.5 w-3.5" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onRemove}
            aria-label={`Remove partition ${partIndex + 1}`}
            className="h-7 w-7 text-destructive hover:text-destructive"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Expanded form */}
      {expanded && (
        <div className="px-3 pb-3 pt-1 border-t space-y-4">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Label */}
            <div className="space-y-2">
              <Label htmlFor={`part-label-${errorPrefix}`}>Label *</Label>
              <Input
                id={`part-label-${errorPrefix}`}
                value={partition.label}
                onChange={(e) => {
                  onClearError?.(`${errorPrefix}.label`);
                  onUpdate({ ...partition, label: e.target.value });
                }}
                placeholder="e.g., boot, root, data"
                aria-invalid={!!labelError}
              />
              <FormFieldError error={labelError} />
            </div>

            {/* Size */}
            <div className="space-y-2">
              <Label htmlFor={`part-size-${errorPrefix}`}>Size *</Label>
              <div className="flex gap-2">
                <Input
                  id={`part-size-${errorPrefix}`}
                  value={partition.size}
                  onChange={(e) => {
                    onClearError?.(`${errorPrefix}.size`);
                    onUpdate({ ...partition, size: e.target.value });
                  }}
                  placeholder="512MiB | 50% | *"
                  aria-invalid={!!sizeError}
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    onClearError?.(`${errorPrefix}.size`);
                    onUpdate({ ...partition, size: "*" });
                  }}
                  className="shrink-0"
                >
                  Rest
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                Fixed: 512MiB, 10G &nbsp;|&nbsp; Percent: 50% &nbsp;|&nbsp; Remaining: *
              </p>
              <FormFieldError error={sizeError} />
            </div>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Filesystem */}
            <div className="space-y-2">
              <Label htmlFor={`part-fs-${errorPrefix}`}>Filesystem</Label>
              <select
                id={`part-fs-${errorPrefix}`}
                value={getFilesystemSelectValue(partition)}
                onChange={(e) => handleFilesystemChange(e.target.value)}
                className={selectClassName}
                aria-invalid={!!filesystemError}
              >
                {FILESYSTEM_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
              <FormFieldError error={filesystemError} />
            </div>

            {/* Mount Point */}
            {showMountPoint && (
              <div className="space-y-2">
                <Label htmlFor={`part-mount-${errorPrefix}`}>Mount Point</Label>
                <Input
                  id={`part-mount-${errorPrefix}`}
                  value={partition.mount_point ?? ""}
                  onChange={(e) => {
                    onClearError?.(`${errorPrefix}.mount_point`);
                    onUpdate({
                      ...partition,
                      mount_point: e.target.value || undefined,
                    });
                  }}
                  placeholder="e.g., /, /boot, /home"
                  aria-invalid={!!mountPointError}
                />
                <FormFieldError error={mountPointError} />
              </div>
            )}
          </div>

          {/* Additional Flags */}
          <div className="space-y-2">
            <Label>Additional Flags</Label>
            <div className="flex flex-wrap gap-4 p-3 border rounded-md bg-muted/30">
              {ADDITIONAL_FLAGS.map((flag) => (
                <label
                  key={flag}
                  className="flex items-center gap-2 cursor-pointer text-sm"
                >
                  <input
                    type="checkbox"
                    checked={partition.flags?.includes(flag) ?? false}
                    onChange={() => handleFlagToggle(flag)}
                    className="h-4 w-4 rounded border-border"
                  />
                  <span className="font-mono">{flag}</span>
                </label>
              ))}
            </div>
            <FormFieldError error={flagsError} />
          </div>

          {/* Volume Group selector — only shown when lvm flag is active */}
          {isLvm && (
            <div className="space-y-2">
              <Label htmlFor={`part-vg-${errorPrefix}`}>Volume Group</Label>
              {vgNames.length === 0 ? (
                <select
                  id={`part-vg-${errorPrefix}`}
                  disabled
                  className={selectClassName}
                  aria-invalid={!!volumeGroupError}
                >
                  <option value="">— Create a Volume Group below</option>
                </select>
              ) : (
                <select
                  id={`part-vg-${errorPrefix}`}
                  value={partition.volume_group ?? ""}
                  onChange={(e) => {
                    onClearError?.(`${errorPrefix}.volume_group`);
                    onUpdate({
                      ...partition,
                      volume_group: e.target.value || undefined,
                    });
                  }}
                  className={selectClassName}
                  aria-invalid={!!volumeGroupError}
                >
                  <option value="">— Select VG</option>
                  {vgNames.map((name) => (
                    <option key={name} value={name}>
                      {name}
                    </option>
                  ))}
                </select>
              )}
              <FormFieldError error={volumeGroupError} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
