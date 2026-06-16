import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Trash2, ChevronDown, ChevronUp } from "lucide-react";
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

const NO_FILESYSTEM = "__none__";

const FILESYSTEM_OPTIONS = [
  ...BASE_FILESYSTEM_OPTIONS,
  { value: "__lvm__", label: "— LVM Physical Volume" },
  { value: NO_FILESYSTEM, label: "— None (raw)" },
];

const ADDITIONAL_FLAGS = ["boot", "esp", "bios_grub"];

function getFilesystemSelectValue(partition: PartitionConfig): string {
  if (partition.flags?.includes("lvm")) return "__lvm__";
  if (partition.filesystem === undefined) return NO_FILESYSTEM;
  return partition.filesystem;
}

function getFilesystemSummary(partition: PartitionConfig): string {
  if (partition.flags?.includes("lvm")) return "LVM PV";
  if (partition.filesystem === undefined) return "raw";
  return partition.filesystem;
}

// Badge for partition flags
function FlagBadge({ flag }: { flag: string }) {
  return (
    <span className="inline-flex items-center px-1.5 py-0 text-[10px] font-medium rounded-sm bg-status-new-bg text-status-new border border-status-new/20">
      {flag}
    </span>
  );
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
  const showMountPoint = !isLvm && partition.filesystem !== undefined && partition.filesystem !== "swap";

  const displayFlags = (partition.flags ?? []).filter((f) => f !== "lvm");
  if (isLvm) displayFlags.unshift("lvm");

  const sizeDisplay =
    partition.size
      ? partition.size === "*" || partition.size === "rest"
        ? "rest"
        : partition.size
      : "—";

  function handleFilesystemChange(val: string) {
    onClearError?.(`${errorPrefix}.filesystem`);
    if (val === "__lvm__") {
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
    } else if (val === NO_FILESYSTEM) {
      const newFlags = (partition.flags ?? []).filter((f) => f !== "lvm");
      onUpdate({
        ...partition,
        filesystem: undefined,
        mount_point: undefined,
        volume_group: undefined,
        flags: newFlags.length > 0 ? newFlags : undefined,
      });
    } else {
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

  return (
    <>
      {/* Collapsed summary row */}
      <tr
        className="border-b border-border-muted last:border-b-0 hover:bg-bg-raised/50 transition-colors"
        style={{ background: "transparent" }}
      >
        <td className="px-3 py-1.5 text-xs text-text-primary font-mono">
          {partition.mount_point || <span className="text-text-muted">—</span>}
        </td>
        <td className="px-3 py-1.5 text-xs text-text-primary font-mono">
          {sizeDisplay}
        </td>
        <td className="px-3 py-1.5 text-xs text-text-secondary">
          {getFilesystemSummary(partition)}
        </td>
        <td className="px-3 py-1.5 text-xs">
          <div className="flex flex-wrap gap-1">
            {displayFlags.map((f) => (
              <FlagBadge key={f} flag={f} />
            ))}
          </div>
        </td>
        <td className="px-3 py-1.5 text-xs">
          <div className="flex items-center gap-1 justify-end">
            <button
              type="button"
              onClick={() => setExpanded((e) => !e)}
              aria-label={expanded ? "Collapse partition" : "Expand partition"}
              className="text-text-muted hover:text-text-primary transition-colors cursor-pointer"
            >
              {expanded ? (
                <ChevronUp className="h-3.5 w-3.5" />
              ) : (
                <ChevronDown className="h-3.5 w-3.5" />
              )}
            </button>
            <button
              type="button"
              onClick={onRemove}
              aria-label={`Remove partition ${partIndex + 1}`}
              className="text-text-muted hover:text-status-broken transition-colors cursor-pointer"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
        </td>
      </tr>

      {/* Expanded edit form row */}
      {expanded && (
        <tr>
          <td colSpan={5} className="px-3 pb-3 pt-2 bg-bg-surface border-b border-border-muted">
            <div className="space-y-3">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {/* Label */}
                <div className="space-y-1">
                  <Label htmlFor={`part-label-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Label *
                  </Label>
                  <Input
                    id={`part-label-${errorPrefix}`}
                    value={partition.label}
                    onChange={(e) => {
                      onClearError?.(`${errorPrefix}.label`);
                      onUpdate({ ...partition, label: e.target.value });
                    }}
                    placeholder="e.g., boot, root, data"
                    aria-invalid={!!labelError}
                    className="h-7 text-xs"
                  />
                  <FormFieldError error={labelError} />
                </div>

                {/* Size */}
                <div className="space-y-1">
                  <Label htmlFor={`part-size-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Size *
                  </Label>
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
                      className="h-7 text-xs flex-1"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        onClearError?.(`${errorPrefix}.size`);
                        onUpdate({ ...partition, size: "*" });
                      }}
                      className="h-7 px-2 text-xs border border-border text-text-secondary hover:text-text-primary hover:border-accent transition-colors rounded-sm cursor-pointer"
                    >
                      Rest
                    </button>
                  </div>
                  <p className="text-xs text-text-muted">
                    Fixed: 512MiB, 10G | Percent: 50% | Remaining: *
                  </p>
                  <FormFieldError error={sizeError} />
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {/* Filesystem */}
                <div className="space-y-1">
                  <Label htmlFor={`part-fs-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Filesystem
                  </Label>
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
                  <div className="space-y-1">
                    <Label htmlFor={`part-mount-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                      Mount Point
                    </Label>
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
                      className="h-7 text-xs"
                    />
                    <FormFieldError error={mountPointError} />
                  </div>
                )}
              </div>

              {/* Additional Flags */}
              <div className="space-y-1">
                <Label className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                  Additional Flags
                </Label>
                <div className="flex flex-wrap gap-4 px-3 py-2 border border-border bg-bg-raised/50 rounded-sm">
                  {ADDITIONAL_FLAGS.map((flag) => (
                    <label
                      key={flag}
                      className="flex items-center gap-2 cursor-pointer text-xs text-text-secondary hover:text-text-primary"
                    >
                      <input
                        type="checkbox"
                        checked={partition.flags?.includes(flag) ?? false}
                        onChange={() => handleFlagToggle(flag)}
                        className="h-3.5 w-3.5 rounded-sm border-border accent-accent"
                      />
                      <span className="font-mono">{flag}</span>
                    </label>
                  ))}
                </div>
                <FormFieldError error={flagsError} />
              </div>

              {/* Volume Group — only when lvm flag is active */}
              {isLvm && (
                <div className="space-y-1">
                  <Label htmlFor={`part-vg-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Volume Group
                  </Label>
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
          </td>
        </tr>
      )}
    </>
  );
}
