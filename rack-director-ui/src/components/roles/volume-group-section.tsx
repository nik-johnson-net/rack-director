import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { FormFieldError } from "@/components/ui/form-field-error";
import { Plus, Trash2, Layers } from "lucide-react";
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

  return (
    <div className="space-y-4">
      {/* Section header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Layers className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium text-foreground">Volume Groups</span>
        </div>
        {!showAddVgInput && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setShowAddVgInput(true)}
          >
            <Plus className="h-4 w-4 mr-2" />
            Add Volume Group
          </Button>
        )}
      </div>

      {/* Inline Add VG form */}
      {showAddVgInput && (
        <div className="flex items-end gap-2 p-3 border rounded-md bg-muted/30">
          <div className="flex-1 space-y-2">
            <Label htmlFor="new-vg-name">Volume Group Name</Label>
            <Input
              id="new-vg-name"
              value={newVgName}
              onChange={(e) => setNewVgName(e.target.value)}
              onKeyDown={handleAddVgKeyDown}
              placeholder="e.g., vg0, data-vg"
              autoFocus
            />
          </div>
          <Button
            type="button"
            size="sm"
            onClick={handleAddVg}
            disabled={!newVgName.trim()}
          >
            Add
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              setShowAddVgInput(false);
              setNewVgName("");
            }}
          >
            Cancel
          </Button>
        </div>
      )}

      {/* Volume Group list */}
      {volumeGroups.length === 0 && !showAddVgInput ? (
        <p className="text-sm text-muted-foreground text-center py-2">
          No volume groups defined. Add a volume group to configure LVM.
        </p>
      ) : (
        <div className="space-y-4">
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
      )}
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
    <Card>
      <CardHeader className="pb-3">
        <div className="flex flex-col sm:flex-row sm:items-start gap-3">
          {/* VG name + PV badges */}
          <div className="flex-1 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <Label
                htmlFor={`vg-name-${vgIndex}`}
                className="text-xs text-muted-foreground shrink-0"
              >
                VG Name:
              </Label>
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
                className="h-8 text-sm w-40"
              />
            </div>
            {nameError && <FormFieldError error={nameError} />}
            {/* Physical volumes feeding this VG */}
            {pvLabels.length > 0 && (
              <div className="flex flex-wrap items-center gap-1">
                <span className="text-xs text-muted-foreground">Physical Volumes:</span>
                {pvLabels.map((label) => (
                  <Badge key={label} variant="secondary" className="text-xs font-mono">
                    {label}
                  </Badge>
                ))}
              </div>
            )}
          </div>

          {/* Remove VG button */}
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={onRemove}
            disabled={isReferenced}
            title={
              isReferenced
                ? "Cannot remove: this VG is referenced by one or more partitions"
                : "Remove volume group"
            }
            aria-label={`Remove volume group ${vg.name}`}
            className="h-7 w-7 shrink-0"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </CardHeader>

      <CardContent className="space-y-3 pt-0">
        {/* Logical volumes */}
        {vg.logical_volumes.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-2">
            No logical volumes defined.
          </p>
        ) : (
          <div className="space-y-2">
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
          </div>
        )}

        {/* Add LV button */}
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleAddLv}
          className="w-full"
        >
          <Plus className="h-4 w-4 mr-2" />
          Add Logical Volume
        </Button>
      </CardContent>
    </Card>
  );
}
