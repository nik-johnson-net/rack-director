-- Add architecture column to devices
ALTER TABLE devices ADD COLUMN architecture TEXT NOT NULL DEFAULT 'x86-64' CHECK(architecture IN ('x86-64'));

CREATE INDEX idx_devices_architecture ON devices(architecture);
