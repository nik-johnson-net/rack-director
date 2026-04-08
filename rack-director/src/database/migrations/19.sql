-- Migrate roles from os_id FK to composite OSM reference.
-- Since there are no production instances, existing roles are dropped and recreated.

DROP TABLE IF EXISTS roles;

CREATE TABLE roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    description TEXT,
    -- OSM module name (e.g., "Default")
    osm_module TEXT NOT NULL,
    -- OS name within the module (e.g., "Ubuntu")
    os_name TEXT NOT NULL,
    -- OS release (e.g., "22.04")
    os_release TEXT NOT NULL,
    -- Architecture (e.g., "x86-64")
    os_arch TEXT NOT NULL,
    disk_layout TEXT NOT NULL,
    cmdline_args TEXT,
    config_template TEXT,
    firmware_mode TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_roles_name ON roles(name);
CREATE INDEX idx_roles_osm ON roles(osm_module, os_name, os_release);

-- Drop legacy operating system tables (no longer needed)
DROP TABLE IF EXISTS os_architectures;
DROP TABLE IF EXISTS operating_systems;
