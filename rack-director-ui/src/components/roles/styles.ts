export const selectClassName =
  "w-full h-8 bg-bg-base border border-border text-text-primary text-xs px-3 py-1.5 rounded-sm focus:outline-none focus:border-accent appearance-none cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed aria-[invalid=true]:border-status-broken";

export const BASE_FILESYSTEM_OPTIONS = [
  { value: "ext4", label: "ext4" },
  { value: "xfs", label: "xfs" },
  { value: "btrfs", label: "btrfs" },
  { value: "vfat", label: "vfat (FAT32)" },
  { value: "swap", label: "swap" },
];
