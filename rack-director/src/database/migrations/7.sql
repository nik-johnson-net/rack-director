-- Remove tftp_server and http_server from dhcp_config
-- These are now configured via CLI arguments
ALTER TABLE dhcp_config DROP COLUMN tftp_server;
ALTER TABLE dhcp_config DROP COLUMN http_server;
