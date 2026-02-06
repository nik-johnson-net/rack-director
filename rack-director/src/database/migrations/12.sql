-- Migration 12: Remove default network if it hasn't been customized
-- This migration deletes the default network and pool created in migration 8
-- if they still have their default values and haven't been modified by the user.

-- Delete the default pool if it still has default values
DELETE FROM dhcp_pools
WHERE network_id = (SELECT id FROM dhcp_networks WHERE name = 'Default' AND subnet = '10.0.0.0/24')
AND name = 'Default Pool'
AND range_start = '10.0.0.100'
AND range_end = '10.0.0.200';

-- Delete the default network if it still has default values and no remaining pools/leases
DELETE FROM dhcp_networks
WHERE name = 'Default'
AND subnet = '10.0.0.0/24'
AND gateway = '10.0.0.1'
AND dns_servers = '["8.8.8.8","8.8.4.4"]'
AND NOT EXISTS (SELECT 1 FROM dhcp_pools WHERE network_id = dhcp_networks.id)
AND NOT EXISTS (SELECT 1 FROM dhcp_leases WHERE network_id = dhcp_networks.id);
