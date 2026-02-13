-- Migration 13: Add platforms table and platform_id to devices
-- Platforms group similar physical devices together, representing common hardware configurations
-- Devices are auto-assigned a Platform after hardware discovery

CREATE TABLE platforms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    description TEXT,
    attributes TEXT NOT NULL DEFAULT '{}', -- JSON: PlatformAttributes
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_platforms_name ON platforms(name);

-- Add platform_id to devices (nullable for backward compatibility)
ALTER TABLE devices ADD COLUMN platform_id INTEGER REFERENCES platforms(id) ON DELETE SET NULL;
CREATE INDEX idx_devices_platform_id ON devices(platform_id);
