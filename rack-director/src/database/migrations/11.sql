-- Migration 11: Convert UUID columns from TEXT to BLOB (16 bytes binary)
--
-- This SQL file creates the new table schemas with BLOB UUID columns.
-- Data population is handled by the Rust post-migration hook.

-- Delete bad data from Bug #1 before conversion
DELETE FROM devices WHERE uuid = '{uuid}';

PRAGMA foreign_keys=OFF;

-- Drop any existing _new tables from failed migrations
DROP TABLE IF EXISTS devices_new;
DROP TABLE IF EXISTS plans_new;
DROP TABLE IF EXISTS lifecycle_transitions_new;
DROP TABLE IF EXISTS dhcp_leases_new;
DROP TABLE IF EXISTS pending_devices_new;

-- Create devices table with BLOB uuid
CREATE TABLE devices_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid BLOB UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    first_seen_at DATETIME DEFAULT 0,
    last_seen_at DATETIME DEFAULT 0,
    attributes JSONB DEFAULT '{}',
    lifecycle TEXT DEFAULT 'new' CHECK(lifecycle IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken')),
    role_id INTEGER REFERENCES roles(id) ON DELETE SET NULL,
    architecture TEXT NOT NULL DEFAULT 'x86-64' CHECK(architecture IN ('x86-64'))
);

-- Create plans table with BLOB device_uuid
CREATE TABLE plans_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_uuid BLOB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'success', 'failed')),
    current_step INTEGER DEFAULT 0,
    total_steps INTEGER NOT NULL,
    actions JSONB NOT NULL,
    error_message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME,
    FOREIGN KEY (device_uuid) REFERENCES devices_new(uuid) ON DELETE CASCADE
);

-- Create lifecycle_transitions table with BLOB device_uuid
CREATE TABLE lifecycle_transitions_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_uuid BLOB NOT NULL,
    from_state TEXT NOT NULL CHECK(from_state IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken')),
    to_state TEXT NOT NULL CHECK(to_state IN ('new', 'unprovisioned', 'provisioned', 'removed', 'broken')),
    plan_id INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    success BOOLEAN,
    error_message TEXT,
    FOREIGN KEY (device_uuid) REFERENCES devices_new(uuid) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans_new(id) ON DELETE SET NULL
);

-- Create dhcp_leases table with BLOB device_uuid
CREATE TABLE dhcp_leases_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac_address TEXT UNIQUE NOT NULL,
    ip_address TEXT NOT NULL,
    device_uuid BLOB,
    lease_start DATETIME NOT NULL,
    lease_end DATETIME NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('offered', 'active', 'expired', 'released')),
    hostname TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    network_id INTEGER REFERENCES dhcp_networks(id) ON DELETE SET NULL,
    FOREIGN KEY (device_uuid) REFERENCES devices_new(uuid) ON DELETE SET NULL
);

-- Create pending_devices table with BLOB device_uuid
CREATE TABLE pending_devices_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac_address TEXT UNIQUE NOT NULL,
    device_uuid BLOB,
    network_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    FOREIGN KEY (network_id) REFERENCES dhcp_networks(id) ON DELETE CASCADE,
    FOREIGN KEY (device_uuid) REFERENCES devices_new(uuid) ON DELETE SET NULL
);

-- Note: Data will be migrated and tables renamed by the Rust post-migration hook
-- Foreign keys remain OFF until after data migration completes
