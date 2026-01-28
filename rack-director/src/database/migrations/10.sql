-- Migration 10: Per-network autodiscovery
-- Add enable_autodiscovery column to dhcp_networks table

ALTER TABLE dhcp_networks ADD COLUMN enable_autodiscovery BOOLEAN NOT NULL DEFAULT 0;
