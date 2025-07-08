-- Create Device table to track servers by UUID
CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    first_seen_at DATETIME DEFAULT 0,
    last_seen_at DATETIME DEFAULT 0
);

-- Create index for UUID lookups
CREATE INDEX IF NOT EXISTS idx_devices_uuid ON devices(uuid);