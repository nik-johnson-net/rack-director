-- Recreate plans table without the status check constraint.
-- The constraint is brittle to maintain as new statuses are added.
CREATE TABLE plans_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_uuid TEXT NOT NULL,
    status TEXT NOT NULL,
    current_step INTEGER DEFAULT 0,
    total_steps INTEGER NOT NULL,
    actions JSONB NOT NULL,
    error_message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME,
    FOREIGN KEY (device_uuid) REFERENCES devices(uuid) ON DELETE CASCADE
);

INSERT INTO plans_new SELECT * FROM plans;
DROP TABLE plans;
ALTER TABLE plans_new RENAME TO plans;

CREATE INDEX IF NOT EXISTS idx_plans_device_uuid ON plans(device_uuid);
CREATE INDEX IF NOT EXISTS idx_plans_status ON plans(status);
CREATE INDEX IF NOT EXISTS idx_plans_active ON plans(device_uuid, status) WHERE status IN ('pending', 'running');
