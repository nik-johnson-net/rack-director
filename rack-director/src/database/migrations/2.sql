-- Create Plans table to track execution plans for devices
CREATE TABLE IF NOT EXISTS plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_uuid TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'success', 'failed')),
    current_step INTEGER DEFAULT 0,
    total_steps INTEGER NOT NULL,
    actions JSONB NOT NULL,
    error_message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME,
    FOREIGN KEY (device_uuid) REFERENCES devices(uuid) ON DELETE CASCADE
);

-- Create index for device UUID lookups
CREATE INDEX IF NOT EXISTS idx_plans_device_uuid ON plans(device_uuid);

-- Create index for status lookups
CREATE INDEX IF NOT EXISTS idx_plans_status ON plans(status);

-- Create index for active plans (pending and running)
CREATE INDEX IF NOT EXISTS idx_plans_active ON plans(device_uuid, status) WHERE status IN ('pending', 'running');