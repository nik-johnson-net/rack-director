-- DHCP lease tracking
CREATE TABLE dhcp_leases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac_address TEXT UNIQUE NOT NULL,
    ip_address TEXT NOT NULL,
    device_uuid TEXT,
    lease_start DATETIME NOT NULL,
    lease_end DATETIME NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('offered', 'active', 'expired', 'released')),
    hostname TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (device_uuid) REFERENCES devices(uuid) ON DELETE SET NULL
);

CREATE INDEX idx_dhcp_mac ON dhcp_leases(mac_address);
CREATE INDEX idx_dhcp_ip ON dhcp_leases(ip_address);
CREATE INDEX idx_dhcp_state ON dhcp_leases(state);
CREATE INDEX idx_dhcp_device ON dhcp_leases(device_uuid);

-- DHCP configuration (singleton table)
CREATE TABLE dhcp_config (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    subnet TEXT NOT NULL,
    range_start TEXT NOT NULL,
    range_end TEXT NOT NULL,
    gateway TEXT NOT NULL,
    dns_servers TEXT NOT NULL,
    lease_duration INTEGER NOT NULL DEFAULT 86400,
    tftp_server TEXT NOT NULL,
    http_server TEXT NOT NULL
);

-- Insert default config
INSERT INTO dhcp_config (id, subnet, range_start, range_end, gateway, dns_servers, tftp_server, http_server)
VALUES (1, '10.0.0.0/24', '10.0.0.100', '10.0.0.200', '10.0.0.1', '["8.8.8.8","8.8.4.4"]', '10.0.0.1', '10.0.0.1');
