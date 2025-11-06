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
