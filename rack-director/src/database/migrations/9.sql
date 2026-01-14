-- Migration 9: Pending devices for lease-based device creation

CREATE TABLE pending_devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac_address TEXT UNIQUE NOT NULL,
    device_uuid TEXT,  -- NULL until device boots
    network_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    FOREIGN KEY (network_id) REFERENCES dhcp_networks(id) ON DELETE CASCADE,
    FOREIGN KEY (device_uuid) REFERENCES devices(uuid) ON DELETE SET NULL
);

CREATE INDEX idx_pending_devices_mac ON pending_devices(mac_address);
CREATE INDEX idx_pending_devices_device_uuid ON pending_devices(device_uuid);
CREATE INDEX idx_pending_devices_completed ON pending_devices(completed_at);
