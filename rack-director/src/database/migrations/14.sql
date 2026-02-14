-- Migration 14: Disk layout schema update
-- The disk_layout column in roles already stores JSON.
-- The new DiskLayout format uses { "disks": [...] } instead of { "partitions": [...] }.
-- This migration is a no-op SQL (data conversion happens in the Rust hook).
-- Placeholder to bump version.
SELECT 1;
