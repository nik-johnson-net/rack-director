-- Migration 8: Multi-network DHCP support with relay agents
-- Creates dhcp_networks, dhcp_pools, dhcp_static_reservations tables
-- Migrates existing dhcp_config to "Default" network

-- Create DHCP Networks table
CREATE TABLE dhcp_networks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    subnet TEXT NOT NULL,
    gateway TEXT NOT NULL,
    dns_servers TEXT NOT NULL,
    lease_duration INTEGER NOT NULL DEFAULT 86400,
    relay_agent_address TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_dhcp_networks_relay ON dhcp_networks(relay_agent_address);

-- Create DHCP Pools table (multiple ranges per network)
CREATE TABLE dhcp_pools (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    network_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    range_start TEXT NOT NULL,
    range_end TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (network_id) REFERENCES dhcp_networks(id) ON DELETE CASCADE,
    UNIQUE(network_id, name)
);
CREATE INDEX idx_dhcp_pools_network ON dhcp_pools(network_id);

-- Create Static Reservations table (MAC to IP mappings per network)
CREATE TABLE dhcp_static_reservations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    network_id INTEGER NOT NULL,
    mac_address TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    hostname TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (network_id) REFERENCES dhcp_networks(id) ON DELETE CASCADE,
    UNIQUE(network_id, mac_address),
    UNIQUE(network_id, ip_address)
);
CREATE INDEX idx_dhcp_static_network ON dhcp_static_reservations(network_id);
CREATE INDEX idx_dhcp_static_mac ON dhcp_static_reservations(mac_address);

-- Add network_id to dhcp_leases table
ALTER TABLE dhcp_leases ADD COLUMN network_id INTEGER REFERENCES dhcp_networks(id) ON DELETE SET NULL;
CREATE INDEX idx_dhcp_leases_network ON dhcp_leases(network_id);

-- Note: Tables are created empty. No default network is created.
-- Users must create networks via the UI or API before DHCP functionality is available.
-- Existing dhcp_config table data is discarded as part of the migration to the new multi-network model.

-- Drop old dhcp_config table
DROP TABLE dhcp_config;
