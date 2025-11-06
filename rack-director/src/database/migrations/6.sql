-- Roles
CREATE TABLE roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    description TEXT,
    os_id INTEGER NOT NULL,
    disk_layout TEXT NOT NULL,  -- JSON object defining partition scheme
    config_template TEXT,       -- JSON object with additional configuration
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (os_id) REFERENCES operating_systems(id) ON DELETE RESTRICT
);

CREATE INDEX idx_roles_os_id ON roles(os_id);
CREATE INDEX idx_roles_name ON roles(name);

-- Add role_id column to devices
ALTER TABLE devices ADD COLUMN role_id INTEGER REFERENCES roles(id) ON DELETE SET NULL;

CREATE INDEX idx_devices_role_id ON devices(role_id);
