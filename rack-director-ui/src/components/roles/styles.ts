export const selectClassName =
  "border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:bg-input/30 flex h-10 w-full rounded-md border bg-transparent px-3 py-2 text-base shadow-xs transition-[color,box-shadow] outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50 md:text-sm";

export const BASE_FILESYSTEM_OPTIONS = [
  { value: "ext4", label: "ext4" },
  { value: "xfs", label: "xfs" },
  { value: "btrfs", label: "btrfs" },
  { value: "vfat", label: "vfat (FAT32)" },
  { value: "swap", label: "swap" },
];
