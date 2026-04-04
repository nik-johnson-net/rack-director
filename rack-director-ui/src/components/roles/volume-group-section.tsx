import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Trash2 } from "lucide-react";
import type { VolumeGroup, LogicalVolume } from "@/lib/client";
import LogicalVolumeRow from "./logical-volume-row";

interface VolumeGroupSectionProps {
  volumeGroups: VolumeGroup[];
  pvMap: Record<string, string[]>;
  vgNames: string[];
  onAddVg: (name: string) => void;
  onRenameVg: (vgIndex: number, newName: string) => void;
  onRemoveVg: (vgIndex: number) => void;
  onAddLv: (vgIndex: number) => void;
  onRemoveLv: (vgIndex: number, lvIndex: number) => void;
  onUpdateLv: (vgIndex: number, lvIndex: number, lv: Partial<LogicalVolume>) => void;
  errors?: Record<string, string>;
  onClearError?: (key: string) => void;
}

export default function VolumeGroupSection({
  volumeGroups,
  pvMap,
  onAddVg,
  onRenameVg,
  onRemoveVg,
  onAddLv,
  onRemoveLv,
  onUpdateLv,
  errors,
  onClearError,
}: VolumeGroupSectionProps) {
  const [showAddVgInput, setShowAddVgInput] = useState(false);
  const [newVgName, setNewVgName] = useState("");

  function handleAddVg() {
    const trimmed = newVgName.trim();
    if (!trimmed) return;
    onAddVg(trimmed);
    setNewVgName("");
    setShowAddVgInput(false);
  }

  function handleAddVgKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault();
      handleAddVg();
    } else if (e.key === "Escape") {
      setShowAddVgInput(false);
      setNewVgName("");
    }
  }

  if (volumeGroups.length === 0 && !showAddVgInput) {
    return (
      <div className="flex items-center justify-between mt-2">
        <span className="text-xs text-text-muted">No volume groups defined.</span>
        <button
          type="button"
          onClick={() => setShowAddVgInput(true)}
          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
        >
          + Add Volume Group
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-3 mt-2">
      {/* Section header */}
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-text-secondary uppercase tracking-[0.5px]">
          Volume Groups
        </span>
        {!showAddVgInput && (
          <button
            type="button"
            onClick={() => setShowAddVgInput(true)}
            className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
          >
            + Add Volume Group
          </button>
        )}
      </div>

      {/* Inline Add VG form */}
      {showAddVgInput && (
        <div className="flex items-end gap-2 px-3 py-2 border border-border bg-bg-raised/50 rounded-sm">
          <div className="flex-1 space-y-1">
            <Label htmlFor="new-vg-name" className="text-xs text-text-secondary uppercase tracking-[0.5px]">
              Volume Group Name
            </Label>
            <Input
              id="new-vg-name"
              value={newVgName}
              onChange={(e) => setNewVgName(e.target.value)}
              onKeyDown={handleAddVgKeyDown}
              placeholder="e.g., vg0, data-vg"
              autoFocus
              className="h-7 text-xs"
            />
          </div>
          <button
            type="button"
            onClick={handleAddVg}
            disabled={!newVgName.trim()}
            className="h-7 px-3 text-xs bg-accent text-bg-base rounded-sm hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
          >
            Add
          </button>
          <button
            type="button"
            onClick={() => {
              setShowAddVgInput(false);
              setNewVgName("");
            }}
            className="h-7 px-3 text-xs border border-border text-text-secondary hover:text-text-primary hover:border-accent transition-colors rounded-sm cursor-pointer"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Volume Group list */}
      <div className="space-y-3">
        {volumeGroups.map((vg, vgIndex) => {
          const pvLabels = pvMap[vg.name] ?? [];
          const isReferenced = pvLabels.length > 0;
          const vgErrorPrefix = `volume_groups.${vgIndex}`;

          return (
            <VolumeGroupCard
              key={vgIndex}
              vg={vg}
              vgIndex={vgIndex}
              pvLabels={pvLabels}
              isReferenced={isReferenced}
              onRename={(newName) => onRenameVg(vgIndex, newName)}
              onRemove={() => onRemoveVg(vgIndex)}
              onAddLv={() => onAddLv(vgIndex)}
              onRemoveLv={(lvIndex) => onRemoveLv(vgIndex, lvIndex)}
              onUpdateLv={(lvIndex, lv) => onUpdateLv(vgIndex, lvIndex, lv)}
              errors={errors}
              errorPrefix={vgErrorPrefix}
              onClearError={onClearError}
            />
          );
        })}
      </div>
    </div>
  );
}

interface VolumeGroupCardProps {
  vg: VolumeGroup;
  vgIndex: number;
  pvLabels: string[];
  isReferenced: boolean;
  onRename: (newName: string) => void;
  onRemove: () => void;
  onAddLv: () => void;
  onRemoveLv: (lvIndex: number) => void;
  onUpdateLv: (lvIndex: number, lv: Partial<LogicalVolume>) => void;
  errors?: Record<string, string>;
  errorPrefix: string;
  onClearError?: (key: string) => void;
}

function VolumeGroupCard({
  vg,
  vgIndex,
  pvLabels,
  isReferenced,
  onRename,
  onRemove,
  onAddLv,
  onRemoveLv,
  onUpdateLv,
  errors,
  errorPrefix,
  onClearError,
}: VolumeGroupCardProps) {
  const [newLvIndices, setNewLvIndices] = useState<Set<number>>(new Set());
  const nameError = errors?.[`${errorPrefix}.name`];

  function handleAddLv() {
    setNewLvIndices((prev) => new Set(prev).add(vg.logical_volumes.length));
    onAddLv();
  }

  return (
    <div className="border border-border">
      {/* VG header */}
      <div className="flex items-center justify-between px-3 py-2 bg-bg-raised border-b border-border">
        <div className="flex items-center gap-3 flex-1 min-w-0">
          <span className="text-xs text-text-secondary uppercase tracking-[0.5px] shrink-0">VG:</span>
          <Input
            id={`vg-name-${vgIndex}`}
            value={vg.name}
            onChange={(e) => {
              onClearError?.(`${errorPrefix}.name`);
              onRename(e.target.value);
            }}
            placeholder="e.g., vg0"
            aria-label={`Volume group ${vgIndex + 1} name`}
            aria-invalid={!!nameError}
            className="h-7 text-xs font-semibold text-accent bg-transparent border-transparent focus:border-border max-w-[160px] px-0 font-mono"
          />
          {/* PV labels */}
          {pvLabels.length > 0 && (
            <div className="flex items-center gap-1">
              <span className="text-xs text-text-muted">pvs:</span>
              {pvLabels.map((label) => (
                <span
                  key={label}
                  className="text-xs font-mono text-text-secondary bg-bg-overlay px-1.5 py-0 rounded-sm border border-border-muted"
                >
                  {label}
                </span>
              ))}
            </div>
          )}
        </div>

        <button
          type="button"
          onClick={onRemove}
          disabled={isReferenced}
          title={
            isReferenced
              ? "Cannot remove: this VG is referenced by one or more partitions"
              : "Remove volume group"
          }
          aria-label={`Remove volume group ${vg.name}`}
          className="text-text-muted hover:text-status-broken transition-colors disabled:opacity-30 disabled:cursor-not-allowed cursor-pointer"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {nameError && (
        <div className="px-3 py-1">
          <FormFieldError error={nameError} />
        </div>
      )}

      {/* Logical volumes table */}
      {vg.logical_volumes.length > 0 && (
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Mount</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Size</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">FS</th>
              <th className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-1">Name</th>
              <th className="px-3 py-1 w-10"></th>
            </tr>
          </thead>
          <tbody>
            {vg.logical_volumes.map((lv, lvIndex) => (
              <LogicalVolumeRow
                key={lvIndex}
                lv={lv}
                lvIndex={lvIndex}
                onUpdate={(updates) => onUpdateLv(lvIndex, updates)}
                onRemove={() => onRemoveLv(lvIndex)}
                errors={errors}
                errorPrefix={`${errorPrefix}.logical_volumes.${lvIndex}`}
                onClearError={onClearError}
                initiallyExpanded={newLvIndices.has(lvIndex)}
              />
            ))}
          </tbody>
        </table>
      )}

      {vg.logical_volumes.length === 0 && (
        <div className="px-3 py-3 text-xs text-text-muted text-center">
          No logical volumes defined.
        </div>
      )}

      {/* Add LV link */}
      <div className="px-3 py-1 border-t border-border-muted">
        <button
          type="button"
          onClick={handleAddLv}
          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
        >
          + Add Logical Volume
        </button>
      </div>
    </div>
  );
}
