-- Add lifecycle field to devices table
ALTER TABLE devices ADD COLUMN lifecycle TEXT DEFAULT 'new' CHECK(lifecycle IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken'));

-- Create lifecycle_transitions table to track lifecycle state changes
CREATE TABLE IF NOT EXISTS lifecycle_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_uuid TEXT NOT NULL,
    from_state TEXT NOT NULL CHECK(from_state IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken')),
    to_state TEXT NOT NULL CHECK(to_state IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken')),
    plan_id INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    success BOOLEAN,
    error_message TEXT,
    FOREIGN KEY (device_uuid) REFERENCES devices(uuid) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE SET NULL
);

-- Create index for device UUID lookups
CREATE INDEX IF NOT EXISTS idx_lifecycle_transitions_device_uuid ON lifecycle_transitions(device_uuid);

-- Create index for active transitions
CREATE INDEX IF NOT EXISTS idx_lifecycle_transitions_active ON lifecycle_transitions(device_uuid) WHERE success IS NULL;

-- Create index for completed transitions
CREATE INDEX IF NOT EXISTS idx_lifecycle_transitions_completed ON lifecycle_transitions(device_uuid, completed_at) WHERE success IS NOT NULL;