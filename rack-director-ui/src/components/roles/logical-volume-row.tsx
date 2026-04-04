import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Trash2, ChevronDown, ChevronUp } from "lucide-react";
import type { LogicalVolume } from "@/lib/client";
import { selectClassName, BASE_FILESYSTEM_OPTIONS } from "./styles";

interface LogicalVolumeRowProps {
  lv: LogicalVolume;
  lvIndex: number;
  onUpdate: (lv: Partial<LogicalVolume>) => void;
  onRemove: () => void;
  errors?: Record<string, string>;
  errorPrefix: string;
  onClearError?: (key: string) => void;
  initiallyExpanded?: boolean;
}

export default function LogicalVolumeRow({
  lv,
  lvIndex,
  onUpdate,
  onRemove,
  errors,
  errorPrefix,
  onClearError,
  initiallyExpanded = false,
}: LogicalVolumeRowProps) {
  const [expanded, setExpanded] = useState(initiallyExpanded);

  const nameError = errors?.[`${errorPrefix}.name`];
  const sizeError = errors?.[`${errorPrefix}.size`];
  const filesystemError = errors?.[`${errorPrefix}.filesystem`];
  const mountPointError = errors?.[`${errorPrefix}.mount_point`];

  const showMountPoint = lv.filesystem !== "swap";
  const sizeDisplay = lv.size || "—";

  return (
    <>
      {/* Collapsed summary row */}
      <tr
        className="border-b border-border-muted last:border-b-0 hover:bg-bg-raised/50 transition-colors"
        style={{ background: "transparent" }}
      >
        <td className="px-3 py-1.5 text-xs text-text-primary font-mono">
          {lv.mount_point || <span className="text-text-muted">—</span>}
        </td>
        <td className="px-3 py-1.5 text-xs text-text-primary font-mono">
          {sizeDisplay}
        </td>
        <td className="px-3 py-1.5 text-xs text-text-secondary">
          {lv.filesystem}
        </td>
        <td className="px-3 py-1.5 text-xs text-text-secondary font-mono">
          {lv.name || <span className="text-text-muted italic">unnamed</span>}
        </td>
        <td className="px-3 py-1.5 text-xs">
          <div className="flex items-center gap-1 justify-end">
            <button
              type="button"
              onClick={() => setExpanded((e) => !e)}
              aria-label={expanded ? "Collapse logical volume" : "Expand logical volume"}
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
              aria-label={`Remove logical volume ${lvIndex + 1}`}
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
                {/* Name */}
                <div className="space-y-1">
                  <Label htmlFor={`lv-name-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Name *
                  </Label>
                  <Input
                    id={`lv-name-${errorPrefix}`}
                    value={lv.name}
                    onChange={(e) => {
                      onClearError?.(`${errorPrefix}.name`);
                      onUpdate({ name: e.target.value });
                    }}
                    placeholder="e.g., root, home, swap"
                    aria-invalid={!!nameError}
                    className="h-7 text-xs"
                  />
                  <FormFieldError error={nameError} />
                </div>

                {/* Size */}
                <div className="space-y-1">
                  <Label htmlFor={`lv-size-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Size *
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      id={`lv-size-${errorPrefix}`}
                      value={lv.size}
                      onChange={(e) => {
                        onClearError?.(`${errorPrefix}.size`);
                        onUpdate({ size: e.target.value });
                      }}
                      placeholder="50G | 100%FREE | *"
                      aria-invalid={!!sizeError}
                      className="h-7 text-xs flex-1"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        onClearError?.(`${errorPrefix}.size`);
                        onUpdate({ size: "100%FREE" });
                      }}
                      className="h-7 px-2 text-xs border border-border text-text-secondary hover:text-text-primary hover:border-accent transition-colors rounded-sm cursor-pointer"
                    >
                      Free
                    </button>
                  </div>
                  <p className="text-xs text-text-muted">
                    Fixed: 50G | All remaining: 100%FREE
                  </p>
                  <FormFieldError error={sizeError} />
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {/* Filesystem */}
                <div className="space-y-1">
                  <Label htmlFor={`lv-fs-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                    Filesystem *
                  </Label>
                  <select
                    id={`lv-fs-${errorPrefix}`}
                    value={lv.filesystem}
                    onChange={(e) => {
                      onClearError?.(`${errorPrefix}.filesystem`);
                      const newFs = e.target.value;
                      onUpdate({
                        filesystem: newFs,
                        mount_point: newFs === "swap" ? undefined : lv.mount_point,
                      });
                    }}
                    className={selectClassName}
                    aria-invalid={!!filesystemError}
                  >
                    {BASE_FILESYSTEM_OPTIONS.map((opt) => (
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
                    <Label htmlFor={`lv-mount-${errorPrefix}`} className="text-xs text-text-secondary uppercase tracking-[0.5px]">
                      Mount Point
                    </Label>
                    <Input
                      id={`lv-mount-${errorPrefix}`}
                      value={lv.mount_point ?? ""}
                      onChange={(e) => {
                        onClearError?.(`${errorPrefix}.mount_point`);
                        onUpdate({ mount_point: e.target.value || undefined });
                      }}
                      placeholder="e.g., /, /home"
                      aria-invalid={!!mountPointError}
                      className="h-7 text-xs"
                    />
                    <FormFieldError error={mountPointError} />
                  </div>
                )}
              </div>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
