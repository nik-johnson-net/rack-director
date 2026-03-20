-- Migration 15: Remove path from PlatformDisk JSON
-- PlatformDisk no longer stores a device path — paths vary by PCIe bus topology, so
-- storing them caused two otherwise-identical servers with different slot assignments to
-- be treated as different platforms. The path field is now stripped from each disk entry
-- in platforms.attributes.
-- Data conversion happens in the Rust post-migration hook (migration_15::strip_disk_paths).
SELECT 1;
