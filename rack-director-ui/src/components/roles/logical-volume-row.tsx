import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Badge } from "@/components/ui/badge";
import { ChevronDown, ChevronUp, Trash2 } from "lucide-react";
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
    <div className="border rounded-md bg-card">
      {/* Collapsed summary row */}
      <div className="flex items-center gap-2 px-3 py-2">
        <div className="flex-1 flex flex-wrap items-center gap-2 min-w-0">
          <span className="font-mono text-sm font-medium truncate">
            {lv.name || <span className="text-muted-foreground italic">unnamed</span>}
          </span>
          <Badge variant="outline" className="text-xs shrink-0">
            {sizeDisplay}
          </Badge>
          <span className="text-xs text-muted-foreground shrink-0">{lv.filesystem}</span>
          {lv.mount_point && (
            <span className="text-xs text-muted-foreground font-mono shrink-0">
              {lv.mount_point}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => setExpanded((e) => !e)}
            aria-label={expanded ? "Collapse logical volume" : "Expand logical volume"}
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
            aria-label={`Remove logical volume ${lvIndex + 1}`}
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
            {/* Name */}
            <div className="space-y-2">
              <Label htmlFor={`lv-name-${errorPrefix}`}>Name *</Label>
              <Input
                id={`lv-name-${errorPrefix}`}
                value={lv.name}
                onChange={(e) => {
                  onClearError?.(`${errorPrefix}.name`);
                  onUpdate({ name: e.target.value });
                }}
                placeholder="e.g., root, home, swap"
                aria-invalid={!!nameError}
              />
              <FormFieldError error={nameError} />
            </div>

            {/* Size */}
            <div className="space-y-2">
              <Label htmlFor={`lv-size-${errorPrefix}`}>Size *</Label>
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
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    onClearError?.(`${errorPrefix}.size`);
                    onUpdate({ size: "100%FREE" });
                  }}
                  className="shrink-0"
                >
                  Free
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                Fixed: 50G &nbsp;|&nbsp; All remaining: 100%FREE
              </p>
              <FormFieldError error={sizeError} />
            </div>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Filesystem */}
            <div className="space-y-2">
              <Label htmlFor={`lv-fs-${errorPrefix}`}>Filesystem *</Label>
              <select
                id={`lv-fs-${errorPrefix}`}
                value={lv.filesystem}
                onChange={(e) => {
                  onClearError?.(`${errorPrefix}.filesystem`);
                  const newFs = e.target.value;
                  // Clear mount_point when switching to swap
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
              <div className="space-y-2">
                <Label htmlFor={`lv-mount-${errorPrefix}`}>Mount Point</Label>
                <Input
                  id={`lv-mount-${errorPrefix}`}
                  value={lv.mount_point ?? ""}
                  onChange={(e) => {
                    onClearError?.(`${errorPrefix}.mount_point`);
                    onUpdate({ mount_point: e.target.value || undefined });
                  }}
                  placeholder="e.g., /, /home"
                  aria-invalid={!!mountPointError}
                />
                <FormFieldError error={mountPointError} />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
