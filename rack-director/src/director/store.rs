use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::lifecycle::DeviceLifecycle;
use crate::operating_systems::Architecture;

#[derive(Debug, Clone)]
pub struct Device {
    pub uuid: String,
    pub architecture: Architecture,
    pub lifecycle: Option<DeviceLifecycle>,
    pub role_id: Option<i64>,
    pub attributes: serde_json::Map<String, serde_json::Value>,
    pub created_at: Option<String>,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub interface_name: String,
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub is_primary: bool,
    /// Network ID this interface is on (if it has an IP)
    #[serde(default)]
    pub network_id: Option<i64>,
    /// Whether this interface is disabled (e.g., due to duplicate MAC)
    #[serde(default)]
    pub disabled: bool,
    /// Warning message explaining why interface is disabled
    #[serde(default)]
    pub warning_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDevice {
    pub id: i64,
    pub mac_address: String,
    pub device_uuid: Option<String>,
    pub network_id: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone)]
pub struct DirectorStore {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
}

impl DirectorStore {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { conn }
    }

    pub async fn register_device(&self, uuid: &str, architecture: Architecture) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
            params![uuid, architecture.as_str()],
        )?;
        Ok(())
    }

    pub async fn device_exists(&self, uuid: &str) -> Result<bool> {
        let conn = self.conn.lock().await;
        let res = conn
            .query_one("SELECT 1 FROM devices WHERE uuid = ?1", [uuid], |r| {
                r.get(0)
            })
            .optional()
            .map(|op: Option<i32>| op.is_some())?;
        Ok(res)
    }

    pub async fn update_device_last_seen(&self, uuid: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE devices SET last_seen_at = CURRENT_TIMESTAMP WHERE uuid = ?1",
            [uuid],
        )?;
        Ok(())
    }

    pub async fn update_attributes(
        &self,
        uuid: &str,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        // Get existing attributes
        let device = self.get_device(uuid).await?;
        let mut merged_attributes = device.attributes;

        // Merge new attributes (new values overwrite existing keys)
        for (key, value) in attributes {
            merged_attributes.insert(key, value);
        }

        // Update with merged attributes
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE devices SET attributes = ?1 WHERE uuid = ?2",
            [&serde_json::to_string(&merged_attributes)?, uuid],
        )?;

        Ok(())
    }

    pub async fn get_device(&self, uuid: &str) -> Result<Device> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid, architecture, lifecycle, role_id, attributes, created_at, first_seen_at, last_seen_at FROM devices WHERE uuid = ?1"
        )?;
        let device = stmt.query_row(params![uuid], |row| {
            let uuid: String = row.get(0)?;
            let architecture_str: String = row.get(1)?;
            let lifecycle_str: Option<String> = row.get(2)?;
            let role_id: Option<i64> = row.get(3)?;
            let attributes_json: Option<String> = row.get(4)?;
            // Timestamps can be stored as either TEXT (ISO 8601) or INTEGER (Unix timestamp)
            // Try to get as string first, if that fails try as integer
            let created_at: Option<String> = row.get(5).ok();
            let first_seen_at: Option<String> = row.get(6).ok();
            let last_seen_at: Option<String> = row.get(7).ok();

            let attributes = match attributes_json {
                Some(json_str) => {
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json_str)
                        .unwrap_or_else(|_| serde_json::Map::new())
                }
                None => serde_json::Map::new(),
            };

            let architecture =
                Architecture::from_str(&architecture_str).unwrap_or(Architecture::X86_64);

            let lifecycle = lifecycle_str.map(DeviceLifecycle::from);

            Ok(Device {
                uuid,
                architecture,
                lifecycle,
                role_id,
                attributes,
                created_at,
                first_seen_at,
                last_seen_at,
            })
        })?;

        Ok(device)
    }

    pub async fn get_all_devices(&self) -> Result<Vec<Device>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid, architecture, lifecycle, role_id, attributes, created_at, first_seen_at, last_seen_at FROM devices"
        )?;
        let rows = stmt.query_map([], |row| {
            let uuid: String = row.get(0)?;
            let architecture_str: String = row.get(1)?;
            let lifecycle_str: Option<String> = row.get(2)?;
            let role_id: Option<i64> = row.get(3)?;
            let attributes_json: Option<String> = row.get(4)?;
            // Timestamps can be stored as either TEXT (ISO 8601) or INTEGER (Unix timestamp)
            // Try to get as string first, if that fails try as integer
            let created_at: Option<String> = row.get(5).ok();
            let first_seen_at: Option<String> = row.get(6).ok();
            let last_seen_at: Option<String> = row.get(7).ok();

            let attributes = match attributes_json {
                Some(json_str) => {
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json_str)
                        .unwrap_or_else(|_| serde_json::Map::new())
                }
                None => serde_json::Map::new(),
            };

            let architecture =
                Architecture::from_str(&architecture_str).unwrap_or(Architecture::X86_64);

            let lifecycle = lifecycle_str.map(DeviceLifecycle::from);

            Ok(Device {
                uuid,
                architecture,
                lifecycle,
                role_id,
                attributes,
                created_at,
                first_seen_at,
                last_seen_at,
            })
        })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row?);
        }

        Ok(devices)
    }

    /// Find device UUID by MAC address from device attributes
    /// Searches both legacy mac_address field and network_interfaces array
    pub async fn find_device_by_mac(&self, mac: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid FROM devices
             WHERE json_extract(attributes, '$.mac_address') = ?
                OR EXISTS (
                  SELECT 1 FROM json_each(attributes, '$.network_interfaces')
                  WHERE json_extract(value, '$.mac_address') = ?
                )",
        )?;

        let result = stmt
            .query_row(params![mac, mac], |row| row.get::<_, String>(0))
            .optional()?;

        Ok(result)
    }

    /// Set hostname in device attributes
    pub async fn set_hostname(&self, uuid: &str, hostname: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.hostname', ?) WHERE uuid = ?",
            params![hostname, uuid],
        )?;

        Ok(())
    }

    /// Set MAC address in device attributes
    pub async fn set_mac_address(&self, uuid: &str, mac: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        // First, update the legacy mac_address field
        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.mac_address', ?) WHERE uuid = ?",
            params![mac, uuid],
        )?;

        // Then, if network_interfaces array exists, update the primary NIC's MAC address
        let has_interfaces: bool = conn
            .query_row(
                "SELECT json_type(attributes, '$.network_interfaces') FROM devices WHERE uuid = ?",
                params![uuid],
                |row| {
                    let json_type: Option<String> = row.get(0)?;
                    Ok(json_type == Some("array".to_string()))
                },
            )
            .optional()?
            .unwrap_or(false);

        if has_interfaces {
            // Find the index of the primary interface
            let primary_index: Option<i64> = conn
                .query_row(
                    "SELECT key FROM json_each((SELECT attributes FROM devices WHERE uuid = ?), '$.network_interfaces')
                     WHERE json_extract(value, '$.is_primary') = 1
                     LIMIT 1",
                    params![uuid],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;

            if let Some(index) = primary_index {
                let path = format!("$.network_interfaces[{}].mac_address", index);
                conn.execute(
                    "UPDATE devices SET attributes = json_set(attributes, ?, ?) WHERE uuid = ?",
                    params![path, mac, uuid],
                )?;
            }
        }

        Ok(())
    }

    /// Set IP address in device attributes (called by DHCP when lease becomes active)
    /// Updates either BMC IP or network interface IP based on the MAC address
    pub async fn set_ip_address(&self, uuid: &str, ip: &str, mac: &str) -> Result<()> {
        // Check if this MAC belongs to the BMC
        let is_bmc: bool = {
            let conn = self.conn.lock().await;
            conn.query_row(
                "SELECT COALESCE(json_extract(attributes, '$.bmc.mac_address') = ?, 0) FROM devices WHERE uuid = ?",
                params![mac, uuid],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false)
        };

        if is_bmc {
            // Update BMC IP address
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE devices SET attributes = json_set(attributes, '$.bmc.ip_address', ?) WHERE uuid = ?",
                params![ip, uuid],
            )?;
            return Ok(());
        }

        // Not a BMC - update network interface by MAC address
        // Get current network interfaces
        let mut interfaces = self.get_network_interfaces(uuid).await?;

        // Find interface with matching MAC
        if let Some(interface) = interfaces.iter_mut().find(|i| i.mac_address == mac) {
            // Update existing interface
            interface.ip_address = Some(ip.to_string());
        } else {
            // MAC not found - create new interface
            interfaces.push(NetworkInterface {
                interface_name: "unknown".to_string(), // Will be updated by agent
                mac_address: mac.to_string(),
                ip_address: Some(ip.to_string()),
                is_primary: interfaces.is_empty(), // Primary only if it's the first interface
                network_id: None,
                disabled: false,
                warning_label: None,
            });
        }

        // Save updated interfaces
        self.set_network_interfaces(uuid, &interfaces).await?;

        Ok(())
    }

    /// Get network interfaces from device attributes
    pub async fn get_network_interfaces(&self, uuid: &str) -> Result<Vec<NetworkInterface>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT json_extract(attributes, '$.network_interfaces') FROM devices WHERE uuid = ?",
        )?;

        let result = stmt
            .query_row(params![uuid], |row| row.get::<_, Option<String>>(0))
            .optional()?;

        match result {
            Some(Some(json_str)) => {
                // Try to parse the JSON array
                let interfaces: Vec<NetworkInterface> =
                    serde_json::from_str(&json_str).unwrap_or_else(|_| Vec::new());
                Ok(interfaces)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Set network interfaces in device attributes
    pub async fn set_network_interfaces(
        &self,
        uuid: &str,
        interfaces: &[NetworkInterface],
    ) -> Result<()> {
        let conn = self.conn.lock().await;

        let json_str = serde_json::to_string(interfaces)?;

        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces', json(?)) WHERE uuid = ?",
            params![json_str, uuid],
        )?;

        Ok(())
    }

    /// Find device UUID by MAC address in either legacy mac_address field or network_interfaces array
    #[cfg(test)]
    pub async fn find_device_by_any_mac(&self, mac: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid FROM devices
             WHERE json_extract(attributes, '$.mac_address') = ?
                OR EXISTS (
                  SELECT 1 FROM json_each(attributes, '$.network_interfaces')
                  WHERE json_extract(value, '$.mac_address') = ?
                )",
        )?;

        let result = stmt
            .query_row(params![mac, mac], |row| row.get::<_, String>(0))
            .optional()?;

        Ok(result)
    }

    /// Create a pending device entry for a MAC address
    /// Returns the ID of the created pending device
    /// If a pending device already exists for this MAC, does nothing and returns the existing ID
    pub async fn create_pending_device(&self, mac_address: &str, network_id: i64) -> Result<i64> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO pending_devices (mac_address, network_id) VALUES (?1, ?2)
             ON CONFLICT(mac_address) DO NOTHING",
            params![mac_address, network_id],
        )?;

        let id = conn.last_insert_rowid();

        // If no rows were inserted (conflict), get the existing ID
        if id == 0 {
            let existing_id: i64 = conn.query_row(
                "SELECT id FROM pending_devices WHERE mac_address = ?1",
                params![mac_address],
                |row| row.get(0),
            )?;
            Ok(existing_id)
        } else {
            Ok(id)
        }
    }

    /// Find pending device ID by MAC address
    /// Returns None if no pending device exists or if it's already completed
    pub async fn find_pending_device_by_mac(&self, mac_address: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().await;

        let result = conn
            .query_row(
                "SELECT id FROM pending_devices WHERE mac_address = ?1 AND completed_at IS NULL",
                params![mac_address],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        Ok(result)
    }

    /// Complete a pending device by linking it to a device UUID
    /// Marks the pending device as completed
    pub async fn complete_pending_device(&self, mac_address: &str, device_uuid: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE pending_devices
             SET device_uuid = ?1, completed_at = CURRENT_TIMESTAMP
             WHERE mac_address = ?2 AND completed_at IS NULL",
            params![device_uuid, mac_address],
        )?;

        Ok(())
    }

    /// Get all pending devices that haven't been completed yet
    pub async fn get_pending_devices(&self) -> Result<Vec<PendingDevice>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, mac_address, device_uuid, network_id, created_at, completed_at
             FROM pending_devices
             WHERE completed_at IS NULL
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(PendingDevice {
                id: row.get(0)?,
                mac_address: row.get(1)?,
                device_uuid: row.get(2)?,
                network_id: row.get(3)?,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
            })
        })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row?);
        }

        Ok(devices)
    }

    /// Delete a pending device by ID
    pub async fn delete_pending_device(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM pending_devices WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Find device UUID by BMC MAC address
    ///
    /// Searches all devices for a BMC with the given MAC address in their attributes.
    /// Returns the device UUID if a match is found.
    pub async fn find_device_by_bmc_mac(&self, mac: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid FROM devices
             WHERE json_extract(attributes, '$.bmc.mac_address') = ?",
        )?;

        let result = stmt
            .query_row(params![mac], |row| row.get::<_, String>(0))
            .optional()?;

        Ok(result)
    }

    /// Find devices with the same MAC address on the same network
    /// Returns Vec<(device_uuid, interface_name)>
    ///
    /// This function searches for duplicate MAC addresses on a specific network,
    /// excluding a given device UUID. It's used to detect MAC conflicts during
    /// device registration.
    pub async fn find_duplicate_macs_on_network(
        &self,
        mac: &str,
        network_id: i64,
        exclude_device: &str,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid, attributes FROM devices
             WHERE uuid != ?1
             AND EXISTS (
               SELECT 1 FROM json_each(attributes, '$.network_interfaces') as iface
               WHERE json_extract(iface.value, '$.mac_address') = ?2
                 AND json_extract(iface.value, '$.network_id') = ?3
             )",
        )?;

        let rows = stmt.query_map(params![exclude_device, mac, network_id], |row| {
            let uuid: String = row.get(0)?;
            let attributes_json: Option<String> = row.get(1)?;
            Ok((uuid, attributes_json))
        })?;

        let mut duplicates = Vec::new();

        for row in rows {
            let (uuid, attributes_json) = row?;

            // Parse attributes to find the matching interface name
            if let Some(json_str) = attributes_json {
                if let Ok(attributes) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json_str) {
                    if let Some(interfaces_value) = attributes.get("network_interfaces") {
                        if let Some(interfaces_array) = interfaces_value.as_array() {
                            for interface_value in interfaces_array {
                                // Parse each interface
                                if let Ok(interface) = serde_json::from_value::<NetworkInterface>(interface_value.clone()) {
                                    if interface.mac_address == mac && interface.network_id == Some(network_id) {
                                        duplicates.push((uuid.clone(), interface.interface_name.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(duplicates)
    }
}

/// Extract last UUID segment (after final hyphen) for hostname generation
pub fn extract_uuid_last_segment(uuid: &str) -> String {
    uuid.split('-').next_back().unwrap_or("unknown").to_string()
}

/// Generate hostname from UUID: "node-{last_segment}"
pub fn generate_hostname_from_uuid(uuid: &str) -> String {
    format!("node-{}", extract_uuid_last_segment(uuid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> (DirectorStore, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(&db_path).unwrap();
        (DirectorStore::new(Arc::new(Mutex::new(conn))), temp_dir)
    }

    #[test]
    fn test_extract_uuid_last_segment() {
        assert_eq!(
            extract_uuid_last_segment("550e8400-e29b-41d4-a716-446655440010"),
            "446655440010"
        );
        assert_eq!(extract_uuid_last_segment("simple-uuid"), "uuid");
        assert_eq!(extract_uuid_last_segment("no-hyphens"), "hyphens");
        assert_eq!(extract_uuid_last_segment("single"), "single");
    }

    #[test]
    fn test_generate_hostname_from_uuid() {
        assert_eq!(
            generate_hostname_from_uuid("550e8400-e29b-41d4-a716-446655440010"),
            "node-446655440010"
        );
        assert_eq!(generate_hostname_from_uuid("simple-uuid"), "node-uuid");
    }

    #[tokio::test]
    async fn test_set_hostname() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440020";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set hostname
        store.set_hostname(uuid, "test-hostname").await.unwrap();

        // Verify
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "test-hostname"
        );
    }

    #[tokio::test]
    async fn test_set_mac_address() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440021";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set MAC address
        store
            .set_mac_address(uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        // Verify
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .get("mac_address")
                .unwrap()
                .as_str()
                .unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
    }

    #[tokio::test]
    async fn test_set_ip_address() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440023";
        let mac = "aa:bb:cc:dd:ee:ff";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set IP address for a MAC (creates new interface)
        store.set_ip_address(uuid, "10.0.0.150", mac).await.unwrap();

        // Verify interface was created with correct IP
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, mac);
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.150".to_string()));
        assert!(interfaces[0].is_primary); // Should be primary as it's the first interface

        // Verify legacy ip_address field is NOT set
        let device = store.get_device(uuid).await.unwrap();
        assert!(device.attributes.get("ip_address").is_none());
    }

    #[tokio::test]
    async fn test_hostname_generation_on_register() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440022";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Generate and set hostname
        let hostname = generate_hostname_from_uuid(uuid);
        store.set_hostname(uuid, &hostname).await.unwrap();

        // Verify
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "node-446655440022"
        );
    }

    #[tokio::test]
    async fn test_update_attributes_preserves_existing() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440024";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set initial attributes (hostname via set_hostname to simulate real flow)
        store.set_hostname(uuid, "server-01").await.unwrap();

        // Verify initial state
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "server-01"
        );

        // Simulate hardware discovery updating attributes
        let mut hardware_attrs = serde_json::Map::new();
        hardware_attrs.insert(
            "manufacturer".to_string(),
            serde_json::Value::String("Dell Inc.".to_string()),
        );
        hardware_attrs.insert(
            "product_name".to_string(),
            serde_json::Value::String("PowerEdge R640".to_string()),
        );
        hardware_attrs.insert(
            "serial_number".to_string(),
            serde_json::Value::String("ABC12345".to_string()),
        );

        store.update_attributes(uuid, hardware_attrs).await.unwrap();

        // Verify ALL attributes are present (both old and new)
        let device = store.get_device(uuid).await.unwrap();

        // Original attribute should be preserved
        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "server-01",
            "hostname should be preserved after update_attributes"
        );

        // New attributes should be added
        assert_eq!(
            device
                .attributes
                .get("manufacturer")
                .unwrap()
                .as_str()
                .unwrap(),
            "Dell Inc."
        );
        assert_eq!(
            device
                .attributes
                .get("product_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "PowerEdge R640"
        );
        assert_eq!(
            device
                .attributes
                .get("serial_number")
                .unwrap()
                .as_str()
                .unwrap(),
            "ABC12345"
        );

        // Total should be 4 attributes
        assert_eq!(device.attributes.len(), 4);
    }

    #[tokio::test]
    async fn test_update_attributes_overwrites_existing_keys() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440025";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set initial attributes
        let mut initial_attrs = serde_json::Map::new();
        initial_attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("old-hostname".to_string()),
        );
        initial_attrs.insert(
            "manufacturer".to_string(),
            serde_json::Value::String("Unknown".to_string()),
        );
        store.update_attributes(uuid, initial_attrs).await.unwrap();

        // Update with overlapping keys
        let mut new_attrs = serde_json::Map::new();
        new_attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("new-hostname".to_string()),
        );
        new_attrs.insert(
            "product_name".to_string(),
            serde_json::Value::String("PowerEdge".to_string()),
        );
        store.update_attributes(uuid, new_attrs).await.unwrap();

        // Verify overlapping key is updated, non-overlapping keys are preserved
        let device = store.get_device(uuid).await.unwrap();

        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "new-hostname",
            "hostname should be updated to new value"
        );
        assert_eq!(
            device
                .attributes
                .get("manufacturer")
                .unwrap()
                .as_str()
                .unwrap(),
            "Unknown",
            "manufacturer should be preserved"
        );
        assert_eq!(
            device
                .attributes
                .get("product_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "PowerEdge",
            "product_name should be added"
        );

        assert_eq!(device.attributes.len(), 3);
    }

    #[tokio::test]
    async fn test_update_attributes_empty_map() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440026";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set initial attributes
        store.set_hostname(uuid, "test-host").await.unwrap();

        // Update with empty map (should preserve existing)
        let empty_attrs = serde_json::Map::new();
        store.update_attributes(uuid, empty_attrs).await.unwrap();

        // Verify existing attributes are preserved
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device.attributes.get("hostname").unwrap().as_str().unwrap(),
            "test-host"
        );
        assert_eq!(device.attributes.len(), 1);
    }

    // Tests for multi-NIC support

    #[tokio::test]
    async fn test_get_network_interfaces_empty() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440030";

        // Register device without any NICs
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Get interfaces should return empty vec
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_get_network_interfaces_single() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440031";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set single network interface
        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
        network_id: None,
        disabled: false,
        warning_label: None,
        }];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Retrieve and verify
        let retrieved = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].interface_name, "eth0");
        assert_eq!(retrieved[0].mac_address, "aa:bb:cc:dd:ee:01");
        assert_eq!(retrieved[0].ip_address, Some("10.0.0.100".to_string()));
        assert!(retrieved[0].is_primary);
    }

    #[tokio::test]
    async fn test_get_network_interfaces_multiple() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440032";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set multiple network interfaces
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth2".to_string(),
                mac_address: "aa:bb:cc:dd:ee:03".to_string(),
                ip_address: None,
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
        ];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Retrieve and verify
        let retrieved = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].interface_name, "eth0");
        assert_eq!(retrieved[1].interface_name, "eth1");
        assert_eq!(retrieved[2].interface_name, "eth2");
        assert!(retrieved[0].is_primary);
        assert!(!retrieved[1].is_primary);
        assert!(!retrieved[2].is_primary);
    }

    #[tokio::test]
    async fn test_set_network_interfaces_overwrites() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440033";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set initial interfaces
        let initial = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
        network_id: None,
        disabled: false,
        warning_label: None,
        }];
        store.set_network_interfaces(uuid, &initial).await.unwrap();

        // Overwrite with different interfaces
        let updated = vec![
            NetworkInterface {
                interface_name: "ens0".to_string(),
                mac_address: "11:22:33:44:55:66".to_string(),
                ip_address: Some("192.168.1.100".to_string()),
                is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "ens1".to_string(),
                mac_address: "11:22:33:44:55:67".to_string(),
                ip_address: None,
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
        ];
        store.set_network_interfaces(uuid, &updated).await.unwrap();

        // Verify it was overwritten
        let retrieved = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].interface_name, "ens0");
        assert_eq!(retrieved[1].interface_name, "ens1");
    }

    #[tokio::test]
    async fn test_find_device_by_mac_legacy_field() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440034";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set MAC using legacy method
        store
            .set_mac_address(uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        // Find by MAC should work
        let found = store.find_device_by_mac("aa:bb:cc:dd:ee:ff").await.unwrap();
        assert_eq!(found, Some(uuid.to_string()));

        // Non-existent MAC should return None
        let not_found = store.find_device_by_mac("00:00:00:00:00:00").await.unwrap();
        assert_eq!(not_found, None);
    }

    #[tokio::test]
    async fn test_find_device_by_mac_in_interfaces_array() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440035";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set network interfaces
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
        ];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Find by primary MAC
        let found1 = store.find_device_by_mac("aa:bb:cc:dd:ee:01").await.unwrap();
        assert_eq!(found1, Some(uuid.to_string()));

        // Find by secondary MAC
        let found2 = store.find_device_by_mac("aa:bb:cc:dd:ee:02").await.unwrap();
        assert_eq!(found2, Some(uuid.to_string()));

        // Non-existent MAC should return None
        let not_found = store.find_device_by_mac("00:00:00:00:00:00").await.unwrap();
        assert_eq!(not_found, None);
    }

    #[tokio::test]
    async fn test_find_device_by_any_mac() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440036";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set both legacy MAC and interfaces array
        store
            .set_mac_address(uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: None,
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
        ];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Find by legacy MAC
        let found_legacy = store
            .find_device_by_any_mac("aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();
        assert_eq!(found_legacy, Some(uuid.to_string()));

        // Find by interface MAC
        let found_iface = store
            .find_device_by_any_mac("aa:bb:cc:dd:ee:02")
            .await
            .unwrap();
        assert_eq!(found_iface, Some(uuid.to_string()));
    }

    #[tokio::test]
    async fn test_set_mac_address_legacy_only() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440037";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set MAC address without interfaces array
        store
            .set_mac_address(uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        // Verify legacy field is set
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .get("mac_address")
                .unwrap()
                .as_str()
                .unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );

        // Verify interfaces array is still empty
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_set_mac_address_updates_primary_interface() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440038";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set network interfaces
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: None,
                is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
            },
        ];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Update MAC address
        store
            .set_mac_address(uuid, "11:22:33:44:55:66")
            .await
            .unwrap();

        // Verify legacy field is updated
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .get("mac_address")
                .unwrap()
                .as_str()
                .unwrap(),
            "11:22:33:44:55:66"
        );

        // Verify primary interface MAC is updated
        let updated_interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(updated_interfaces[0].mac_address, "11:22:33:44:55:66");
        // Secondary interface should be unchanged
        assert_eq!(updated_interfaces[1].mac_address, "aa:bb:cc:dd:ee:02");
    }

    #[tokio::test]
    async fn test_set_ip_address_creates_interface_when_missing() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440039";
        let mac = "aa:bb:cc:dd:ee:ff";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set IP address without pre-existing interfaces array
        store.set_ip_address(uuid, "10.0.0.100", mac).await.unwrap();

        // Verify interface was created
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, mac);
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.100".to_string()));
        assert!(interfaces[0].is_primary);

        // Verify legacy field is NOT set
        let device = store.get_device(uuid).await.unwrap();
        assert!(device.attributes.get("ip_address").is_none());
    }

    #[tokio::test]
    async fn test_set_ip_address_updates_by_mac() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440040";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set network interfaces
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
                network_id: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                is_primary: false,
                network_id: None,
                disabled: false,
                warning_label: None,
            },
        ];
        store
            .set_network_interfaces(uuid, &interfaces)
            .await
            .unwrap();

        // Update IP address for eth1 (non-primary) by MAC
        store
            .set_ip_address(uuid, "192.168.1.50", "aa:bb:cc:dd:ee:02")
            .await
            .unwrap();

        // Verify eth1 IP is updated
        let updated_interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(
            updated_interfaces[1].ip_address,
            Some("192.168.1.50".to_string())
        );
        // Primary interface (eth0) should be unchanged
        assert_eq!(
            updated_interfaces[0].ip_address,
            Some("10.0.0.100".to_string())
        );

        // Verify legacy field is NOT set
        let device = store.get_device(uuid).await.unwrap();
        assert!(device.attributes.get("ip_address").is_none());
    }

    #[tokio::test]
    async fn test_backward_compatibility_legacy_device() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440041";

        // Register device and set up as a legacy device (no network_interfaces)
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        store
            .set_mac_address(uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();
        store
            .set_ip_address(uuid, "10.0.0.100", "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        // Verify legacy mac_address field still works
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .get("mac_address")
                .unwrap()
                .as_str()
                .unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );

        // New behavior: ip_address is stored in network_interfaces, not legacy field
        assert!(device.attributes.get("ip_address").is_none());

        // Verify find_device_by_mac still works
        let found = store.find_device_by_mac("aa:bb:cc:dd:ee:ff").await.unwrap();
        assert_eq!(found, Some(uuid.to_string()));

        // Verify network_interfaces was created with the IP
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.100".to_string()));
    }

    #[tokio::test]
    async fn test_set_ip_address_for_bmc() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440042";
        let bmc_mac = "aa:bb:cc:dd:ee:aa";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set BMC information in device attributes
        let conn = store.conn.lock().await;
        conn.execute(
            r#"UPDATE devices SET attributes = json_set(attributes, '$.bmc',
               json('{"mac_address":"aa:bb:cc:dd:ee:aa","ip_address":null,"ip_address_source":"Unknown"}')
            ) WHERE uuid = ?"#,
            params![uuid],
        )
        .unwrap();
        drop(conn);

        // Set IP address for BMC MAC
        store
            .set_ip_address(uuid, "10.0.1.50", bmc_mac)
            .await
            .unwrap();

        // Verify BMC IP was updated
        let device = store.get_device(uuid).await.unwrap();
        let bmc = device.attributes.get("bmc").unwrap();
        assert_eq!(
            bmc.get("ip_address").unwrap().as_str().unwrap(),
            "10.0.1.50"
        );

        // Verify network_interfaces was NOT created
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_get_network_interfaces_invalid_json() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440042";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Manually set invalid JSON in network_interfaces field
        let conn = store.conn.lock().await;
        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces', 'invalid') WHERE uuid = ?",
            params![uuid],
        ).unwrap();
        drop(conn);

        // Should return empty vec instead of error
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    // Tests for duplicate MAC detection

    #[tokio::test]
    async fn test_network_interface_disabled_fields_serialization() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440043";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Create interface with new fields
        let interface = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: true,
            warning_label: Some("Duplicate MAC on network main".to_string()),
        };

        store
            .set_network_interfaces(uuid, &[interface.clone()])
            .await
            .unwrap();

        // Retrieve and verify all fields
        let retrieved = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].network_id, Some(1));
        assert_eq!(retrieved[0].disabled, true);
        assert_eq!(
            retrieved[0].warning_label,
            Some("Duplicate MAC on network main".to_string())
        );
    }

    #[tokio::test]
    async fn test_network_interface_backward_compatibility() {
        let (store, _temp) = create_test_store().await;
        let uuid = "550e8400-e29b-41d4-a716-446655440044";

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Manually set old-style interface without new fields
        let conn = store.conn.lock().await;
        conn.execute(
            r#"UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces',
               json('[{"interface_name":"eth0","mac_address":"aa:bb:cc:dd:ee:01","ip_address":"10.0.0.100","is_primary":true}]')
            ) WHERE uuid = ?"#,
            params![uuid],
        )
        .unwrap();
        drop(conn);

        // Should deserialize with default values for new fields
        let interfaces = store.get_network_interfaces(uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].network_id, None);
        assert_eq!(interfaces[0].disabled, false);
        assert_eq!(interfaces[0].warning_label, None);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_network_no_duplicates() {
        let (store, _temp) = create_test_store().await;
        let uuid1 = "550e8400-e29b-41d4-a716-446655440045";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440046";

        // Register two devices
        store
            .register_device(uuid1, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid2, Architecture::X86_64)
            .await
            .unwrap();

        // Set different MACs on same network
        let interface1 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        let interface2 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:02".to_string(),
            ip_address: Some("10.0.0.101".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        store
            .set_network_interfaces(uuid1, &[interface1])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid2, &[interface2])
            .await
            .unwrap();

        // Should find no duplicates
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:01", 1, uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_network_finds_duplicate() {
        let (store, _temp) = create_test_store().await;
        let uuid1 = "550e8400-e29b-41d4-a716-446655440047";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440048";

        // Register two devices
        store
            .register_device(uuid1, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid2, Architecture::X86_64)
            .await
            .unwrap();

        // Set SAME MAC on same network
        let interface1 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:99".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        let interface2 = NetworkInterface {
            interface_name: "ens0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:99".to_string(),
            ip_address: Some("10.0.0.101".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        store
            .set_network_interfaces(uuid1, &[interface1])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid2, &[interface2])
            .await
            .unwrap();

        // Should find duplicate when checking from uuid1
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:99", 1, uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].0, uuid2);
        assert_eq!(duplicates[0].1, "ens0");

        // Should find duplicate when checking from uuid2
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:99", 1, uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].0, uuid1);
        assert_eq!(duplicates[0].1, "eth0");
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_different_networks() {
        let (store, _temp) = create_test_store().await;
        let uuid1 = "550e8400-e29b-41d4-a716-446655440049";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440050";

        // Register two devices
        store
            .register_device(uuid1, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid2, Architecture::X86_64)
            .await
            .unwrap();

        // Set SAME MAC on DIFFERENT networks
        let interface1 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:88".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        let interface2 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:88".to_string(),
            ip_address: Some("192.168.1.100".to_string()),
            is_primary: true,
            network_id: Some(2),
            disabled: false,
            warning_label: None,
        };

        store
            .set_network_interfaces(uuid1, &[interface1])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid2, &[interface2])
            .await
            .unwrap();

        // Should NOT find duplicate on network 1
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:88", 1, uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);

        // Should NOT find duplicate on network 2
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:88", 2, uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_multiple_duplicates() {
        let (store, _temp) = create_test_store().await;
        let uuid1 = "550e8400-e29b-41d4-a716-446655440051";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440052";
        let uuid3 = "550e8400-e29b-41d4-a716-446655440053";

        // Register three devices
        store
            .register_device(uuid1, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid2, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid3, Architecture::X86_64)
            .await
            .unwrap();

        // All three have same MAC on same network
        let interface1 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:77".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        let interface2 = NetworkInterface {
            interface_name: "ens0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:77".to_string(),
            ip_address: Some("10.0.0.101".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        let interface3 = NetworkInterface {
            interface_name: "enp0s3".to_string(),
            mac_address: "aa:bb:cc:dd:ee:77".to_string(),
            ip_address: Some("10.0.0.102".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        store
            .set_network_interfaces(uuid1, &[interface1])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid2, &[interface2])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid3, &[interface3])
            .await
            .unwrap();

        // Should find 2 duplicates when checking from uuid1
        let mut duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:77", 1, uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 2);

        // Sort for deterministic testing
        duplicates.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(duplicates[0].0, uuid2);
        assert_eq!(duplicates[0].1, "ens0");
        assert_eq!(duplicates[1].0, uuid3);
        assert_eq!(duplicates[1].1, "enp0s3");
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_no_network_id() {
        let (store, _temp) = create_test_store().await;
        let uuid1 = "550e8400-e29b-41d4-a716-446655440054";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440055";

        // Register two devices
        store
            .register_device(uuid1, Architecture::X86_64)
            .await
            .unwrap();
        store
            .register_device(uuid2, Architecture::X86_64)
            .await
            .unwrap();

        // Set same MAC but without network_id (legacy interface)
        let interface1 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:66".to_string(),
            ip_address: None,
            is_primary: true,
            network_id: None,
            disabled: false,
            warning_label: None,
        };

        let interface2 = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:66".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: true,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        };

        store
            .set_network_interfaces(uuid1, &[interface1])
            .await
            .unwrap();
        store
            .set_network_interfaces(uuid2, &[interface2])
            .await
            .unwrap();

        // Should NOT find uuid1 (no network_id) when searching network 1
        let duplicates = store
            .find_duplicate_macs_on_network("aa:bb:cc:dd:ee:66", 1, uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_pending_device() {
        let (store, _temp) = create_test_store().await;
        let mac = "aa:bb:cc:dd:ee:99";
        let network_id = 1;

        // Create a pending device
        let pending_id = store
            .create_pending_device(mac, network_id)
            .await
            .unwrap();

        // Verify it was created
        let pending_devices = store.get_pending_devices().await.unwrap();
        assert_eq!(pending_devices.len(), 1);
        assert_eq!(pending_devices[0].id, pending_id);
        assert_eq!(pending_devices[0].mac_address, mac);

        // Delete the pending device
        store.delete_pending_device(pending_id).await.unwrap();

        // Verify it was deleted
        let pending_devices = store.get_pending_devices().await.unwrap();
        assert_eq!(pending_devices.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_pending_device() {
        let (store, _temp) = create_test_store().await;

        // Deleting a non-existent pending device should not error
        // (SQL DELETE on non-existent row succeeds with 0 rows affected)
        let result = store.delete_pending_device(999).await;
        assert!(result.is_ok());
    }
}
