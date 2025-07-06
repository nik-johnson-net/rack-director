-- Create interfaces table to track network interfaces
CREATE TABLE IF NOT EXISTS interfaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id INTEGER NOT NULL,
    mac_address TEXT UNIQUE NOT NULL,
    ipv4_address TEXT,
    ipv6_address TEXT,
    is_bmc BOOLEAN DEFAULT FALSE,
    rack_identifier TEXT,
    rack_port TEXT,
    subnet_id INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
);

-- Create subnets table to manage IP address pools
CREATE TABLE IF NOT EXISTS subnets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    network_ipv4 TEXT,
    network_ipv6 TEXT,
    subnet_mask_ipv4 TEXT,
    prefix_length_ipv6 INTEGER,
    gateway_ipv4 TEXT,
    gateway_ipv6 TEXT,
    dns_servers TEXT, -- JSON array of DNS servers
    lease_time INTEGER DEFAULT 3600,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Create IP address leases table to track assigned addresses
CREATE TABLE IF NOT EXISTS dhcp_leases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    interface_id INTEGER NOT NULL,
    subnet_id INTEGER NOT NULL,
    ip_address TEXT NOT NULL,
    lease_start DATETIME DEFAULT CURRENT_TIMESTAMP,
    lease_end DATETIME,
    is_active BOOLEAN DEFAULT TRUE,
    FOREIGN KEY (interface_id) REFERENCES interfaces(id) ON DELETE CASCADE,
    FOREIGN KEY (subnet_id) REFERENCES subnets(id) ON DELETE CASCADE
);

-- Create indices for efficient lookups
CREATE INDEX IF NOT EXISTS idx_interfaces_mac ON interfaces(mac_address);
CREATE INDEX IF NOT EXISTS idx_interfaces_device_id ON interfaces(device_id);
CREATE INDEX IF NOT EXISTS idx_interfaces_bmc ON interfaces(is_bmc);
CREATE INDEX IF NOT EXISTS idx_interfaces_rack ON interfaces(rack_identifier, rack_port);
CREATE INDEX IF NOT EXISTS idx_dhcp_leases_interface_id ON dhcp_leases(interface_id);
CREATE INDEX IF NOT EXISTS idx_dhcp_leases_subnet_id ON dhcp_leases(subnet_id);
CREATE INDEX IF NOT EXISTS idx_dhcp_leases_ip_address ON dhcp_leases(ip_address);
CREATE INDEX IF NOT EXISTS idx_dhcp_leases_active ON dhcp_leases(is_active);