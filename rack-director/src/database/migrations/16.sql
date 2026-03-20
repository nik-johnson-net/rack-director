-- Migration 16: Add device_warnings table
-- Stores non-fatal warnings for devices that surface in the UI, such as
-- stale disk label overrides that reference paths no longer present on the device.
CREATE TABLE IF NOT EXISTS device_warnings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
