-- Migration 24: Make dhcp_networks.gateway nullable
-- SQLite cannot drop NOT NULL via ALTER TABLE, so we rebuild the table.
-- All other columns, constraints, and indexes are preserved exactly.

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
