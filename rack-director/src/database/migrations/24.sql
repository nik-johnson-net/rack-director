-- Migration 24: Make dhcp_networks.gateway nullable
-- SQLite cannot drop NOT NULL via ALTER TABLE, so we rebuild the table.
-- All other columns, constraints, and indexes are preserved exactly.
--
-- dhcp_networks is the PARENT of dhcp_pools, dhcp_static_reservations, and
-- pending_devices (all ON DELETE CASCADE) and dhcp_leases (ON DELETE SET NULL).
-- Unlike migration 11's table rebuild, this migration does NOT recreate those
-- child tables — they stay live throughout. If foreign key enforcement is ON when
-- `DROP TABLE dhcp_networks` below runs, SQLite performs an implicit DELETE FROM
-- dhcp_networks first, which cascades into those child tables and permanently
-- deletes/nulls their rows.
--
-- The guard for this is NOT a `PRAGMA foreign_keys=OFF;` statement in this file:
-- that pragma is a documented no-op while a transaction is pending
-- (https://www.sqlite.org/pragma.html#pragma_foreign_keys), and every migration
-- version here runs inside its own explicit transaction (see `perform_migrations`
-- in src/database/mod.rs). Instead, `FK_GUARDED_MIGRATIONS` in mod.rs disables
-- foreign key enforcement on the connection before this migration's transaction
-- opens and restores it after the transaction closes, which is the only point at
-- which the pragma actually takes effect. See mod.rs for details.

-- Create replacement table with nullable gateway
CREATE TABLE dhcp_networks_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    subnet TEXT NOT NULL,
    gateway TEXT,
    dns_servers TEXT NOT NULL DEFAULT '[]',
    lease_duration INTEGER NOT NULL DEFAULT 86400,
    relay_agent_address TEXT,
    enable_autodiscovery INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Copy all rows (existing non-null gateways remain as-is)
INSERT INTO dhcp_networks_new
    (id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at)
SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
FROM dhcp_networks;

-- Swap tables
DROP TABLE dhcp_networks;
ALTER TABLE dhcp_networks_new RENAME TO dhcp_networks;

-- Restore the index that was on the original table
CREATE INDEX idx_dhcp_networks_relay ON dhcp_networks(relay_agent_address);
