use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
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
    pub async fn find_device_by_mac(&self, mac: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT uuid FROM devices WHERE json_extract(attributes, '$.mac_address') = ?",
        )?;

        let result = stmt
            .query_row(params![mac], |row| row.get::<_, String>(0))
            .optional()?;

        Ok(result)
    }

    /// Get static IP for device from attributes
    pub async fn get_device_static_ip(&self, uuid: &str) -> Result<Option<Ipv4Addr>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT json_extract(attributes, '$.static_ip') FROM devices WHERE uuid = ?",
        )?;

        let result = stmt
            .query_row(params![uuid], |row| row.get::<_, Option<String>>(0))
            .optional()?;

        if let Some(Some(ip_str)) = result {
            Ok(Some(ip_str.parse()?))
        } else {
            Ok(None)
        }
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

        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.mac_address', ?) WHERE uuid = ?",
            params![mac, uuid],
        )?;

        Ok(())
    }

    /// Set IP address in device attributes (called by DHCP when lease becomes active)
    pub async fn set_ip_address(&self, uuid: &str, ip: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.ip_address', ?) WHERE uuid = ?",
            params![ip, uuid],
        )?;

        Ok(())
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

        // Register device
        store
            .register_device(uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set IP address
        store.set_ip_address(uuid, "10.0.0.150").await.unwrap();

        // Verify
        let device = store.get_device(uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .get("ip_address")
                .unwrap()
                .as_str()
                .unwrap(),
            "10.0.0.150"
        );
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
}
