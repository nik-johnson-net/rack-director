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

-- Migrate existing dhcp_config to "Default" network
INSERT INTO dhcp_networks (id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address)
SELECT 1, 'Default', subnet, gateway, dns_servers, lease_duration, NULL
FROM dhcp_config WHERE id = 1;

-- Create "Default Pool" from existing range
INSERT INTO dhcp_pools (network_id, name, range_start, range_end)
SELECT 1, 'Default Pool', range_start, range_end
FROM dhcp_config WHERE id = 1;

-- Migrate static IPs from device attributes to static reservations
INSERT INTO dhcp_static_reservations (network_id, mac_address, ip_address)
SELECT
    1,
    json_extract(attributes, '$.mac_address'),
    json_extract(attributes, '$.static_ip')
FROM devices
WHERE json_extract(attributes, '$.static_ip') IS NOT NULL
  AND json_extract(attributes, '$.mac_address') IS NOT NULL;

-- Link existing leases to default network
UPDATE dhcp_leases SET network_id = 1;

-- Drop old dhcp_config table
DROP TABLE dhcp_config;
