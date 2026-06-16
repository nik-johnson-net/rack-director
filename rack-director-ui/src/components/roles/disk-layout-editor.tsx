import { useReducer, useMemo, useEffect, useRef } from "react";
import { HardDrive } from "lucide-react";
import type {
  DiskLayout,
  DiskConfig,
  FirmwareMode,
  PartitionConfig,
  VolumeGroup,
  LogicalVolume,
} from "@/lib/client";
import DiskSection from "./disk-section";
import VolumeGroupSection from "./volume-group-section";

interface DiskLayoutEditorProps {
  value: DiskLayout;
  onChange: (layout: DiskLayout) => void;
  errors?: Record<string, string>;
  onClearError?: (key: string) => void;
  firmwareMode?: FirmwareMode;
}

type Action =
  | { type: "ADD_DISK" }
  | { type: "REMOVE_DISK"; diskIndex: number }
  | { type: "UPDATE_DISK_LABEL"; diskIndex: number; label: string }
  | { type: "UPDATE_DISK_TABLE"; diskIndex: number; table: string }
  | { type: "ADD_PARTITION"; diskIndex: number }
  | { type: "REMOVE_PARTITION"; diskIndex: number; partIndex: number }
  | { type: "UPDATE_PARTITION"; diskIndex: number; partIndex: number; partition: PartitionConfig }
  | { type: "PREPEND_PARTITION"; diskIndex: number; partition: PartitionConfig }
  | { type: "ADD_VG"; name: string }
  | { type: "RENAME_VG"; vgIndex: number; newName: string }
  | { type: "REMOVE_VG"; vgIndex: number }
  | { type: "ADD_LV"; vgIndex: number }
  | { type: "REMOVE_LV"; vgIndex: number; lvIndex: number }
  | { type: "UPDATE_LV"; vgIndex: number; lvIndex: number; lv: Partial<LogicalVolume> }
  | { type: "SET_WIPE_ALL_DISKS"; value: boolean };

const DEFAULT_DISK_LABELS = ["ROOT", "DATA1", "DATA2", "DATA3", "DATA4"];

function getNextDefaultLabel(disks: DiskConfig[]): string {
  const existing = disks.map((d) => d.device);
  for (const label of DEFAULT_DISK_LABELS) {
    if (!existing.includes(label)) return label;
  }
  return `DATA${disks.length}`;
}

function reducer(state: DiskLayout, action: Action): DiskLayout {
  switch (action.type) {
    case "ADD_DISK": {
      const nextLabel = getNextDefaultLabel(state.disks);
      const newDisk: DiskConfig = {
        device: nextLabel,
        partition_table: "gpt",
        partitions: [],
      };
      return { ...state, disks: [...state.disks, newDisk] };
    }

    case "REMOVE_DISK": {
      const newDisks = state.disks.filter((_, i) => i !== action.diskIndex);
      return { ...state, disks: newDisks };
    }

    case "UPDATE_DISK_LABEL": {
      const newDisks = state.disks.map((disk, i) =>
        i === action.diskIndex ? { ...disk, device: action.label } : disk
      );
      return { ...state, disks: newDisks };
    }

    case "UPDATE_DISK_TABLE": {
      const newDisks = state.disks.map((disk, i) =>
        i === action.diskIndex ? { ...disk, partition_table: action.table } : disk
      );
      return { ...state, disks: newDisks };
    }

    case "ADD_PARTITION": {
      const newDisks = state.disks.map((disk, i) => {
        if (i !== action.diskIndex) return disk;
        const newPartition: PartitionConfig = {
          label: `part${disk.partitions.length + 1}`,
          size: "",
          filesystem: "ext4",
        };
        return { ...disk, partitions: [...disk.partitions, newPartition] };
      });
      return { ...state, disks: newDisks };
    }

    case "REMOVE_PARTITION": {
      const newDisks = state.disks.map((disk, i) => {
        if (i !== action.diskIndex) return disk;
        return {
          ...disk,
          partitions: disk.partitions.filter((_, pi) => pi !== action.partIndex),
        };
      });
      return { ...state, disks: newDisks };
    }

    case "UPDATE_PARTITION": {
      const incoming = action.partition;
      const isLvm = incoming.flags?.includes("lvm") ?? false;
      const vgName = incoming.volume_group;

      let newVolumeGroups = state.volume_groups ? [...state.volume_groups] : [];
      if (isLvm && vgName && !newVolumeGroups.find((vg) => vg.name === vgName)) {
        newVolumeGroups = [...newVolumeGroups, { name: vgName, logical_volumes: [] }];
      }

      const newDisks = state.disks.map((disk, i) => {
        if (i !== action.diskIndex) return disk;
        const newPartitions = disk.partitions.map((part, pi) => {
          if (pi !== action.partIndex) return part;
          if (!isLvm) {
            return { ...incoming, volume_group: undefined };
          }
          return incoming;
        });
        return { ...disk, partitions: newPartitions };
      });

      return {
        ...state,
        disks: newDisks,
        volume_groups: newVolumeGroups.length > 0 ? newVolumeGroups : undefined,
      };
    }

    case "PREPEND_PARTITION": {
      const newDisks = state.disks.map((disk, i) => {
        if (i !== action.diskIndex) return disk;
        return { ...disk, partitions: [action.partition, ...disk.partitions] };
      });
      return { ...state, disks: newDisks };
    }

    case "ADD_VG": {
      const existing = state.volume_groups ?? [];
      if (existing.find((vg) => vg.name === action.name)) return state;
      const newVg: VolumeGroup = { name: action.name, logical_volumes: [] };
      return { ...state, volume_groups: [...existing, newVg] };
    }

    case "RENAME_VG": {
      const vgs = state.volume_groups ?? [];
      const oldName = vgs[action.vgIndex]?.name;
      if (!oldName) return state;

      const newVgs = vgs.map((vg, i) =>
        i === action.vgIndex ? { ...vg, name: action.newName } : vg
      );

      const newDisks = state.disks.map((disk) => ({
        ...disk,
        partitions: disk.partitions.map((part) =>
          part.volume_group === oldName
            ? { ...part, volume_group: action.newName }
            : part
        ),
      }));

      return { ...state, disks: newDisks, volume_groups: newVgs };
    }

    case "REMOVE_VG": {
      const vgs = state.volume_groups ?? [];
      const newVgs = vgs.filter((_, i) => i !== action.vgIndex);
      return {
        ...state,
        volume_groups: newVgs.length > 0 ? newVgs : undefined,
      };
    }

    case "ADD_LV": {
      const vgs = state.volume_groups ?? [];
      const newVgs = vgs.map((vg, i) => {
        if (i !== action.vgIndex) return vg;
        const newLv: LogicalVolume = {
          name: `lv${vg.logical_volumes.length + 1}`,
          size: "",
          filesystem: "ext4",
        };
        return { ...vg, logical_volumes: [...vg.logical_volumes, newLv] };
      });
      return { ...state, volume_groups: newVgs };
    }

    case "REMOVE_LV": {
      const vgs = state.volume_groups ?? [];
      const newVgs = vgs.map((vg, i) => {
        if (i !== action.vgIndex) return vg;
        return {
          ...vg,
          logical_volumes: vg.logical_volumes.filter((_, li) => li !== action.lvIndex),
        };
      });
      return { ...state, volume_groups: newVgs };
    }

    case "UPDATE_LV": {
      const vgs = state.volume_groups ?? [];
      const newVgs = vgs.map((vg, i) => {
        if (i !== action.vgIndex) return vg;
        const newLvs = vg.logical_volumes.map((lv, li) =>
          li === action.lvIndex ? { ...lv, ...action.lv } : lv
        );
        return { ...vg, logical_volumes: newLvs };
      });
      return { ...state, volume_groups: newVgs };
    }

    case "SET_WIPE_ALL_DISKS":
      return { ...state, wipe_all_disks: action.value || undefined };

    default:
      return state;
  }
}

export default function DiskLayoutEditor({
  value,
  onChange,
  errors,
  onClearError,
  firmwareMode,
}: DiskLayoutEditorProps) {
  const [layout, dispatch] = useReducer(reducer, value);
  const isFirstRender = useRef(true);

  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false;
      return;
    }
    onChange(layout);
  }, [layout, onChange]);

  const vgNames = useMemo(
    () => (layout.volume_groups ?? []).map((vg) => vg.name),
    [layout.volume_groups]
  );

  const pvMap = useMemo<Record<string, string[]>>(() => {
    const map: Record<string, string[]> = {};
    for (const disk of layout.disks) {
      for (const part of disk.partitions) {
        if (part.flags?.includes("lvm") && part.volume_group) {
          const vgName = part.volume_group;
          if (!map[vgName]) map[vgName] = [];
          map[vgName].push(part.label);
        }
      }
    }
    return map;
  }, [layout.disks]);

  const globalDiskLayoutError = errors?.["disk_layout"];

  return (
    <div className="space-y-3">
      {/* Global disk_layout error */}
      {globalDiskLayoutError && (
        <div className="px-3 py-2 border border-error-border bg-error-bg text-status-broken text-xs rounded-sm">
          {globalDiskLayoutError}
        </div>
      )}

      {/* Disk blocks */}
      {layout.disks.length === 0 ? (
        <div className="border border-border py-8 text-center">
          <HardDrive className="h-8 w-8 mx-auto text-text-muted mb-3 opacity-50" />
          <p className="text-sm text-text-primary mb-1">No disks defined</p>
          <p className="text-xs text-text-secondary mb-3">
            Add a disk to get started with your disk layout.
          </p>
          <button
            type="button"
            onClick={() => dispatch({ type: "ADD_DISK" })}
            className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
          >
            + Add Device
          </button>
        </div>
      ) : (
        <>
          {layout.disks.map((disk, diskIndex) => (
            <DiskSection
              key={diskIndex}
              disk={disk}
              diskIndex={diskIndex}
              vgNames={vgNames}
              canRemove={layout.disks.length > 1}
              onUpdateLabel={(label) =>
                dispatch({ type: "UPDATE_DISK_LABEL", diskIndex, label })
              }
              onUpdateTable={(table) =>
                dispatch({ type: "UPDATE_DISK_TABLE", diskIndex, table })
              }
              onRemove={() => dispatch({ type: "REMOVE_DISK", diskIndex })}
              onAddPartition={() => dispatch({ type: "ADD_PARTITION", diskIndex })}
              onRemovePartition={(partIndex) =>
                dispatch({ type: "REMOVE_PARTITION", diskIndex, partIndex })
              }
              onUpdatePartition={(partIndex, partition) =>
                dispatch({ type: "UPDATE_PARTITION", diskIndex, partIndex, partition })
              }
              errors={errors}
              errorPrefix={`disks.${diskIndex}`}
              onClearError={onClearError}
              firmwareMode={firmwareMode}
              onPrependPartition={(partition) =>
                dispatch({ type: "PREPEND_PARTITION", diskIndex, partition })
              }
            />
          ))}
          <button
            type="button"
            onClick={() => dispatch({ type: "ADD_DISK" })}
            className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
          >
            + Add Device
          </button>
        </>
      )}

      {/* Wipe all disks */}
      <label className="flex items-center gap-2 text-xs text-text-secondary cursor-pointer">
        <input
          type="checkbox"
          checked={layout.wipe_all_disks ?? false}
          onChange={(e) =>
            dispatch({ type: "SET_WIPE_ALL_DISKS", value: e.target.checked })
          }
          className="accent-[var(--color-accent)]"
        />
        Wipe partition info from ALL disks (not just those listed above)
      </label>

      {/* Volume Groups */}
      <VolumeGroupSection
        volumeGroups={layout.volume_groups ?? []}
        pvMap={pvMap}
        vgNames={vgNames}
        onAddVg={(name) => dispatch({ type: "ADD_VG", name })}
        onRenameVg={(vgIndex, newName) =>
          dispatch({ type: "RENAME_VG", vgIndex, newName })
        }
        onRemoveVg={(vgIndex) => dispatch({ type: "REMOVE_VG", vgIndex })}
        onAddLv={(vgIndex) => dispatch({ type: "ADD_LV", vgIndex })}
        onRemoveLv={(vgIndex, lvIndex) =>
          dispatch({ type: "REMOVE_LV", vgIndex, lvIndex })
        }
        onUpdateLv={(vgIndex, lvIndex, lv) =>
          dispatch({ type: "UPDATE_LV", vgIndex, lvIndex, lv })
        }
        errors={errors}
        onClearError={onClearError}
      />
    </div>
  );
}
