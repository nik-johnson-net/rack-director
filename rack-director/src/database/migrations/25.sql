-- Migration 25: Add optional domain_name to dhcp_networks.
-- Plain ADD COLUMN: no table rebuild, so no FK_GUARDED_MIGRATIONS entry is needed.
ALTER TABLE dhcp_networks ADD COLUMN domain_name TEXT;
