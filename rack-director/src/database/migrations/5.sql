-- Operating Systems
CREATE TABLE operating_systems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, version)
);

CREATE INDEX idx_os_name ON operating_systems(name);

-- OS Architecture-specific configurations
CREATE TABLE os_architectures (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    os_id INTEGER NOT NULL,
    architecture TEXT NOT NULL CHECK(architecture IN ('x86-64')),
    kernel_path TEXT NOT NULL,
    initramfs_path TEXT NOT NULL,
    modules TEXT NOT NULL DEFAULT '[]',  -- JSON array of module paths
    cmdline_args TEXT,
    install_script_path TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(os_id, architecture),
    FOREIGN KEY (os_id) REFERENCES operating_systems(id) ON DELETE CASCADE
);

CREATE INDEX idx_os_arch_os_id ON os_architectures(os_id);
CREATE INDEX idx_os_arch_architecture ON os_architectures(architecture);

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

-- Add architecture column to devices
ALTER TABLE devices ADD COLUMN architecture TEXT NOT NULL DEFAULT 'x86-64' CHECK(architecture IN ('x86-64'));

CREATE INDEX idx_devices_architecture ON devices(architecture);
