use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::{Connection, FromRow};
use crate::device_warnings;
use crate::director::Architecture;
use crate::lifecycle::DeviceLifecycle;
use common::device_attributes::{DeviceAttributes, NetworkInterface};

#[derive(Debug, Clone)]
pub struct Device {
    /// Integer primary key from the `devices` table.
    ///
    /// Used internally when creating device warnings (which reference `device_id`).
    /// Not exposed in HTTP responses.
    pub id: i64,
    pub uuid: Uuid,
    pub architecture: Architecture,
    pub lifecycle: Option<DeviceLifecycle>,
    pub role_id: Option<i64>,
    pub platform_id: Option<i64>,
    pub attributes: DeviceAttributes,
    pub created_at: Option<String>,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

impl FromRow for Device {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let id: i64 = row.get("id")?;
        let uuid = row.get("uuid")?;
        let architecture_str: String = row.get("architecture")?;
        let lifecycle_str: Option<String> = row.get("lifecycle")?;
        let role_id: Option<i64> = row.get("role_id")?;
        let platform_id: Option<i64> = row.get("platform_id")?;
        let attributes_json: Option<String> = row.get("attributes")?;

        // Timestamps can be stored as either TEXT (ISO 8601) or INTEGER (Unix timestamp)
        // Try to get as string first, if that fails try as integer
        let created_at: Option<String> = row.get("created_at").ok();
        let first_seen_at: Option<String> = row.get("first_seen_at").ok();
        let last_seen_at: Option<String> = row.get("last_seen_at").ok();

        let attributes = match attributes_json {
            Some(json_str) => {
                serde_json::from_str::<DeviceAttributes>(&json_str).unwrap_or_default()
            }
            None => DeviceAttributes::default(),
        };

        let architecture =
            Architecture::from_str(&architecture_str).unwrap_or(Architecture::X86_64);
        let lifecycle = lifecycle_str.map(DeviceLifecycle::from);

        Ok(Device {
            id,
            uuid,
            architecture,
            lifecycle,
            role_id,
            platform_id,
            attributes,
            created_at,
            first_seen_at,
            last_seen_at,
        })
    }
}

impl TryFrom<Row<'_>> for Device {
    type Error = rusqlite::Error;

    fn try_from(value: Row<'_>) -> std::result::Result<Self, Self::Error> {
        Self::from_row(&value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDevice {
    pub id: i64,
    pub mac_address: String,
    pub device_uuid: Option<Uuid>,
    pub network_id: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

impl FromRow for PendingDevice {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PendingDevice {
            id: row.get("id")?,
            mac_address: row.get("mac_address")?,
            device_uuid: row.get("device_uuid")?,
            network_id: row.get("network_id")?,
            created_at: row.get("created_at")?,
            completed_at: row.get("completed_at")?,
        })
    }
}

pub async fn register_device(
    conn: &Connection,
    uuid: &Uuid,
    architecture: Architecture,
) -> Result<()> {
    conn.execute(
        "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
        (*uuid, architecture.as_str().to_string()),
    )
    .await
    .map(|_| ())?;
    Ok(())
}

pub async fn device_exists(conn: &Connection, uuid: &Uuid) -> Result<bool> {
    let res = conn
        .query_one("SELECT 1 FROM devices WHERE uuid = ?1", (*uuid,), |r| {
            r.get::<_, i32>(0)
        })
        .await
        .optional()
        .map(|op: Option<i32>| op.is_some())?;
    Ok(res)
}

pub async fn update_device_last_seen(conn: &Connection, uuid: &Uuid) -> Result<()> {
    conn.execute(
        "UPDATE devices SET last_seen_at = CURRENT_TIMESTAMP WHERE uuid = ?1",
        (*uuid,),
    )
    .await?;
    Ok(())
}

pub async fn update_attributes(
    conn: &Connection,
    uuid: &Uuid,
    attributes: serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    // Detect hardware scan updates before consuming the attributes map.
    // A disk scan is identified by the presence of the `disks` key.
    let is_disk_scan = attributes.contains_key("disks");

    let device = get_device(conn, uuid).await?;

    let mut existing_json = serde_json::to_value(&device.attributes)?;
    let existing_map = existing_json.as_object_mut().unwrap();

    for (key, value) in attributes {
        existing_map.insert(key, value);
    }

    let mut merged: DeviceAttributes = serde_json::from_value(existing_json)?;

    // When the incoming update contains a `disks` list (i.e. a hardware scan), validate
    // that every disk label override still references a path present on the device.
    // Stale overrides are removed and a DeviceWarning is created for each one.
    if is_disk_scan {
        let stale = collect_stale_overrides(&merged);
        if !stale.is_empty() {
            drop_stale_overrides(conn, &mut merged, &stale, device.id).await?;
        }
    }

    conn.execute(
        "UPDATE devices SET attributes = ?1 WHERE uuid = ?2",
        (serde_json::to_string(&merged)?, *uuid),
    )
    .await?;

    Ok(())
}

/// Identify labels whose override path is no longer present in the disk list.
///
/// Returns a `Vec` of `(label, path)` pairs that should be dropped.
fn collect_stale_overrides(attrs: &DeviceAttributes) -> Vec<(String, String)> {
    let current_paths: std::collections::HashSet<&str> = attrs
        .disks
        .iter()
        .filter_map(|d| d.path.as_deref())
        .collect();

    attrs
        .disk_label_overrides
        .iter()
        .filter(|(_, path)| !current_paths.contains(path.as_str()))
        .map(|(label, path)| (label.clone(), path.clone()))
        .collect()
}

/// Remove stale label overrides from `attrs` and create a `DeviceWarning` for each one.
async fn drop_stale_overrides(
    conn: &Connection,
    attrs: &mut DeviceAttributes,
    stale: &[(String, String)],
    device_id: i64,
) -> Result<()> {
    for (label, path) in stale {
        attrs.disk_label_overrides.remove(label);
        let message = format!(
            "Label override '{}' references missing disk path '{}', override removed",
            label, path
        );
        device_warnings::create_warning(conn, device_id, "LABEL_OVERRIDE_DROPPED", &message)
            .await?;
        log::warn!("device id={}: {}", device_id, message);
    }
    Ok(())
}

pub async fn get_device(conn: &Connection, uuid: &Uuid) -> Result<Device> {
    let device = conn
        .query_one(
            "SELECT id, uuid, architecture, lifecycle, role_id, platform_id, attributes, created_at, first_seen_at, last_seen_at FROM devices WHERE uuid = ?1",
            (*uuid,),
            Device::from_row,
        )
        .await?;

    Ok(device)
}

pub async fn get_all_devices(conn: &Connection) -> Result<Vec<Device>> {
    let devices = conn
        .query(
            "SELECT id, uuid, architecture, lifecycle, role_id, platform_id, attributes, created_at, first_seen_at, last_seen_at FROM devices",
            (),
            Device::from_row,
        )
        .await?;

    Ok(devices)
}

/// Find device UUID by MAC address from device attributes.
///
/// Searches both legacy mac_address field and network_interfaces array.
pub async fn find_device_by_mac(conn: &Connection, mac: &str) -> Result<Option<Uuid>> {
    let mac = mac.to_string();
    let result = conn
        .query_row(
            "SELECT uuid FROM devices
             WHERE json_extract(attributes, '$.mac_address') = ?1
                OR EXISTS (
                  SELECT 1 FROM json_each(attributes, '$.network_interfaces')
                  WHERE json_extract(value, '$.mac_address') = ?1
                )",
            (mac,),
            |row| row.get(0),
        )
        .await
        .optional()?;

    Ok(result)
}

/// Set hostname in device attributes.
pub async fn set_hostname(conn: &Connection, uuid: &Uuid, hostname: &str) -> Result<()> {
    conn.execute(
        "UPDATE devices SET attributes = json_set(attributes, '$.hostname', ?1) WHERE uuid = ?2",
        (hostname.to_string(), *uuid),
    )
    .await?;

    Ok(())
}

/// Set MAC address in device attributes.
pub async fn set_mac_address(conn: &Connection, uuid: &Uuid, mac: &str) -> Result<()> {
    conn.execute(
        "UPDATE devices SET attributes = json_set(attributes, '$.mac_address', ?1) WHERE uuid = ?2",
        (mac.to_string(), *uuid),
    )
    .await?;

    let uuid_copy = *uuid;
    let has_interfaces: bool = conn
        .query_row(
            "SELECT json_type(attributes, '$.network_interfaces') FROM devices WHERE uuid = ?1",
            (*uuid,),
            |row| {
                let json_type: Option<String> = row.get(0)?;
                Ok(json_type == Some("array".to_string()))
            },
        )
        .await
        .optional()?
        .unwrap_or(false);

    if has_interfaces {
        let first_index: Option<i64> = conn
            .query_row(
                "SELECT key FROM json_each((SELECT attributes FROM devices WHERE uuid = ?1), '$.network_interfaces')
                 LIMIT 1",
                (uuid_copy,),
                |row| row.get::<_, i64>(0),
            )
            .await
            .optional()?;

        if let Some(index) = first_index {
            let path = format!("$.network_interfaces[{}].mac_address", index);
            conn.execute(
                "UPDATE devices SET attributes = json_set(attributes, ?1, ?2) WHERE uuid = ?3",
                (path, mac.to_string(), uuid_copy),
            )
            .await?;
        }
    }

    Ok(())
}

/// Set IP address in device attributes (called by DHCP when lease becomes active).
///
/// Updates either BMC IP or network interface IP based on the MAC address.
pub async fn set_ip_address(conn: &Connection, uuid: &Uuid, ip: &str, mac: &str) -> Result<()> {
    let mac_str = mac.to_string();
    let is_bmc: bool = conn
        .query_row(
            "SELECT COALESCE(json_extract(attributes, '$.bmc.mac_address') = ?1, 0) FROM devices WHERE uuid = ?2",
            (mac_str, *uuid),
            |row| row.get::<_, bool>(0),
        )
        .await
        .optional()?
        .unwrap_or(false);

    if is_bmc {
        conn.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.bmc.ip_address', ?1) WHERE uuid = ?2",
            (ip.to_string(), *uuid),
        )
        .await?;
        return Ok(());
    }

    let mut interfaces = get_network_interfaces(conn, uuid).await?;

    if let Some(interface) = interfaces.iter_mut().find(|i| i.mac_address == mac) {
        interface.ip_address = Some(ip.to_string());
    } else {
        interfaces.push(NetworkInterface {
            interface_name: "unknown".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some(ip.to_string()),
            network_id: None,
            speed_mbps: None,
            disabled: false,
            warning_label: None,
        });
    }

    set_network_interfaces(conn, uuid, &interfaces).await?;

    Ok(())
}

/// Get network interfaces from device attributes.
pub async fn get_network_interfaces(
    conn: &Connection,
    uuid: &Uuid,
) -> Result<Vec<NetworkInterface>> {
    let result = conn
        .query_row(
            "SELECT json_extract(attributes, '$.network_interfaces') FROM devices WHERE uuid = ?1",
            (*uuid,),
            |row| row.get::<_, Option<String>>(0),
        )
        .await
        .optional()?;

    match result {
        Some(Some(json_str)) => {
            let interfaces: Vec<NetworkInterface> =
                serde_json::from_str(&json_str).unwrap_or_else(|_| Vec::new());
            Ok(interfaces)
        }
        _ => Ok(Vec::new()),
    }
}

/// Set network interfaces in device attributes.
pub async fn set_network_interfaces(
    conn: &Connection,
    uuid: &Uuid,
    interfaces: &[NetworkInterface],
) -> Result<()> {
    let json_str = serde_json::to_string(interfaces)?;

    conn.execute(
        "UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces', json(?1)) WHERE uuid = ?2",
        (json_str, *uuid),
    )
    .await?;

    Ok(())
}

/// Create a pending device entry for a MAC address.
///
/// Returns the ID of the created pending device. If a pending device already exists
/// for this MAC, does nothing and returns the existing ID.
pub async fn create_pending_device(
    conn: &Connection,
    mac_address: &str,
    network_id: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO pending_devices (mac_address, network_id) VALUES (?1, ?2)
         ON CONFLICT(mac_address) DO NOTHING",
        (mac_address.to_string(), network_id),
    )
    .await?;

    let id = conn.last_insert_rowid().await;

    if id == 0 {
        let existing_id: i64 = conn
            .query_one(
                "SELECT id FROM pending_devices WHERE mac_address = ?1",
                (mac_address.to_string(),),
                |row| row.get(0),
            )
            .await?;
        Ok(existing_id)
    } else {
        Ok(id)
    }
}

/// Find pending device ID by MAC address.
///
/// Returns None if no pending device exists or if it's already completed.
pub async fn find_pending_device_by_mac(
    conn: &Connection,
    mac_address: &str,
) -> Result<Option<i64>> {
    let result = conn
        .query_row(
            "SELECT id FROM pending_devices WHERE mac_address = ?1 AND completed_at IS NULL",
            (mac_address.to_string(),),
            |row| row.get::<_, i64>(0),
        )
        .await
        .optional()?;

    Ok(result)
}

/// Complete a pending device by linking it to a device UUID.
///
/// Marks the pending device as completed.
pub async fn complete_pending_device(
    conn: &Connection,
    mac_address: &str,
    device_uuid: &Uuid,
) -> Result<()> {
    conn.execute(
        "UPDATE pending_devices
         SET device_uuid = ?1, completed_at = CURRENT_TIMESTAMP
         WHERE mac_address = ?2 AND completed_at IS NULL",
        (*device_uuid, mac_address.to_string()),
    )
    .await?;

    Ok(())
}

/// Get all pending devices that haven't been completed yet.
pub async fn get_pending_devices(conn: &Connection) -> Result<Vec<PendingDevice>> {
    let devices = conn
        .query(
            "SELECT id, mac_address, device_uuid, network_id, created_at, completed_at
             FROM pending_devices
             WHERE completed_at IS NULL
             ORDER BY created_at DESC",
            (),
            PendingDevice::from_row,
        )
        .await?;

    Ok(devices)
}

/// Delete a pending device by ID.
pub async fn delete_pending_device(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM pending_devices WHERE id = ?1", (id,))
        .await?;
    Ok(())
}

/// Delete a device by UUID.
///
/// Cascades to plans and transitions, sets leases device_uuid to NULL.
pub async fn delete_device(conn: &Connection, uuid: &Uuid) -> Result<()> {
    conn.execute(
        "DELETE FROM pending_devices WHERE device_uuid = ?1",
        (*uuid,),
    )
    .await?;
    conn.execute("DELETE FROM devices WHERE uuid = ?1", (*uuid,))
        .await?;
    Ok(())
}

/// Find device UUID by BMC MAC address.
///
/// Searches all devices for a BMC with the given MAC address in their attributes.
/// Returns the device UUID if a match is found.
pub async fn find_device_by_bmc_mac(conn: &Connection, mac: &str) -> Result<Option<Uuid>> {
    let result = conn
        .query_row(
            "SELECT uuid FROM devices
             WHERE json_extract(attributes, '$.bmc.mac_address') = ?1",
            (mac.to_string(),),
            |row| row.get(0),
        )
        .await
        .optional()?;

    Ok(result)
}

/// Assign a platform to a device.
pub async fn assign_platform_to_device(
    conn: &Connection,
    device_uuid: &Uuid,
    platform_id: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE devices SET platform_id = ?1 WHERE uuid = ?2",
        (platform_id, *device_uuid),
    )
    .await
    .context("Failed to assign platform to device")?;

    Ok(())
}

/// Get the platform ID assigned to a device.
pub async fn get_device_platform_id(conn: &Connection, device_uuid: &Uuid) -> Result<Option<i64>> {
    let result = conn
        .query_row(
            "SELECT platform_id FROM devices WHERE uuid = ?1",
            (*device_uuid,),
            |row| row.get::<_, Option<i64>>(0),
        )
        .await
        .optional()?;

    Ok(result.flatten())
}

/// List all devices with a specific platform.
pub async fn list_devices_with_platform(conn: &Connection, platform_id: i64) -> Result<Vec<Uuid>> {
    let uuids = conn
        .query(
            "SELECT uuid FROM devices WHERE platform_id = ?1 ORDER BY uuid",
            (platform_id,),
            |row| row.get(0),
        )
        .await?;

    Ok(uuids)
}

/// Assign a role to a device.
pub async fn assign_role_to_device(
    conn: &Connection,
    device_uuid: &Uuid,
    role_id: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE devices SET role_id = ?1 WHERE uuid = ?2",
        (role_id, *device_uuid),
    )
    .await
    .context("Failed to assign role to device")?;

    Ok(())
}

/// Get the role ID assigned to a device.
pub async fn get_device_role_id(conn: &Connection, device_uuid: &Uuid) -> Result<Option<i64>> {
    let result = conn
        .query_row(
            "SELECT role_id FROM devices WHERE uuid = ?1",
            (*device_uuid,),
            |row| row.get::<_, Option<i64>>(0),
        )
        .await
        .optional()?;

    Ok(result.flatten())
}

/// List all devices with a specific role.
pub async fn list_devices_with_role(conn: &Connection, role_id: i64) -> Result<Vec<Uuid>> {
    let uuids = conn
        .query(
            "SELECT uuid FROM devices WHERE role_id = ?1 ORDER BY uuid",
            (role_id,),
            |row| row.get(0),
        )
        .await?;

    Ok(uuids)
}

/// Find devices with the same MAC address on the same network.
///
/// Returns Vec<(device_uuid, interface_name)>. This function searches for duplicate MAC
/// addresses on a specific network, excluding a given device UUID. It's used to detect
/// MAC conflicts during device registration.
pub async fn find_duplicate_macs_on_network(
    conn: &Connection,
    mac: &str,
    network_id: i64,
    exclude_device: &Uuid,
) -> Result<Vec<(Uuid, String)>> {
    let mac = mac.to_string();
    let exclude = *exclude_device;

    let rows = conn
        .query(
            "SELECT uuid, attributes FROM devices
             WHERE uuid != ?1
             AND EXISTS (
               SELECT 1 FROM json_each(attributes, '$.network_interfaces') as iface
               WHERE json_extract(iface.value, '$.mac_address') = ?2
                 AND json_extract(iface.value, '$.network_id') = ?3
             )",
            (exclude, mac.clone(), network_id),
            |row| {
                let uuid: Uuid = row.get(0)?;
                let attributes_json: Option<String> = row.get(1)?;
                Ok((uuid, attributes_json))
            },
        )
        .await?;

    let mut duplicates = Vec::new();

    for (uuid, attributes_json) in rows {
        if let Some(json_str) = attributes_json
            && let Ok(attributes) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json_str)
            && let Some(interfaces_value) = attributes.get("network_interfaces")
            && let Some(interfaces_array) = interfaces_value.as_array()
        {
            for interface_value in interfaces_array {
                if let Ok(interface) =
                    serde_json::from_value::<NetworkInterface>(interface_value.clone())
                    && interface.mac_address == mac
                    && interface.network_id == Some(network_id)
                {
                    duplicates.push((uuid, interface.interface_name.clone()));
                }
            }
        }
    }

    Ok(duplicates)
}

/// Extract last UUID segment (after final hyphen) for hostname generation.
pub fn extract_uuid_last_segment(uuid: &Uuid) -> String {
    let uuid_str = uuid.to_string();
    uuid_str
        .split('-')
        .next_back()
        .unwrap_or("unknown")
        .to_string()
}

/// Generate hostname from UUID: "node-{last_segment}".
pub fn generate_hostname_from_uuid(uuid: &Uuid) -> String {
    format!("node-{}", extract_uuid_last_segment(uuid))
}

#[cfg(test)]
mod tests {
    use crate::test_database_path;

    use super::*;
    use uuid::Uuid;

    fn test_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-4466554400{:02x}", suffix))
            .expect("test UUID should be valid")
    }

    async fn setup_db(path: String) -> Connection {
        let factory =
            crate::database::DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        crate::database::run_migrations(&factory).await.unwrap()
    }

    /// Helper to create a test network and pool for tests that need DHCP functionality.
    async fn create_test_network(conn: &Connection) -> i64 {
        let network = crate::dhcp::store::create_network(
            conn,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[test]
    fn test_extract_uuid_last_segment() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440010").unwrap();
        assert_eq!(extract_uuid_last_segment(&uuid), "446655440010");
    }

    #[test]
    fn test_generate_hostname_from_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440010").unwrap();
        assert_eq!(generate_hostname_from_uuid(&uuid), "node-446655440010");
    }

    #[tokio::test]
    async fn test_set_hostname() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x20);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_hostname(&db, &uuid, "test-hostname").await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "test-hostname"
        );
    }

    #[tokio::test]
    async fn test_set_mac_address() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x21);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_mac_address(&db, &uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.mac_address.as_ref().unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
    }

    #[tokio::test]
    async fn test_set_ip_address() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x23);
        let mac = "aa:bb:cc:dd:ee:ff";

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_ip_address(&db, &uuid, "10.0.0.150", mac).await.unwrap();

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, mac);
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.150".to_string()));

        let device = get_device(&db, &uuid).await.unwrap();
        assert!(device.attributes.static_ip.is_none());
    }

    #[tokio::test]
    async fn test_hostname_generation_on_register() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x22);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        let hostname = generate_hostname_from_uuid(&uuid);
        set_hostname(&db, &uuid, &hostname).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "node-446655440022"
        );
    }

    #[tokio::test]
    async fn test_update_attributes_preserves_existing() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x24);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_hostname(&db, &uuid, "server-01").await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(device.attributes.hostname.as_ref().unwrap(), "server-01");

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

        update_attributes(&db, &uuid, hardware_attrs).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "server-01",
            "hostname should be preserved after update_attributes"
        );
        assert_eq!(
            device.attributes.manufacturer.as_ref().unwrap(),
            "Dell Inc."
        );
        assert_eq!(
            device.attributes.product_name.as_ref().unwrap(),
            "PowerEdge R640"
        );
        assert_eq!(
            device.attributes.serial_number.as_ref().unwrap(),
            "ABC12345"
        );
    }

    #[tokio::test]
    async fn test_update_attributes_overwrites_existing_keys() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x25);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let mut initial_attrs = serde_json::Map::new();
        initial_attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("old-hostname".to_string()),
        );
        initial_attrs.insert(
            "manufacturer".to_string(),
            serde_json::Value::String("Unknown".to_string()),
        );
        update_attributes(&db, &uuid, initial_attrs).await.unwrap();

        let mut new_attrs = serde_json::Map::new();
        new_attrs.insert(
            "hostname".to_string(),
            serde_json::Value::String("new-hostname".to_string()),
        );
        new_attrs.insert(
            "product_name".to_string(),
            serde_json::Value::String("PowerEdge".to_string()),
        );
        update_attributes(&db, &uuid, new_attrs).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.hostname.as_ref().unwrap(),
            "new-hostname",
            "hostname should be updated to new value"
        );
        assert_eq!(
            device.attributes.manufacturer.as_ref().unwrap(),
            "Unknown",
            "manufacturer should be preserved"
        );
        assert_eq!(
            device.attributes.product_name.as_ref().unwrap(),
            "PowerEdge",
            "product_name should be added"
        );
    }

    #[tokio::test]
    async fn test_update_attributes_empty_map() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x26);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_hostname(&db, &uuid, "test-host").await.unwrap();

        let empty_attrs = serde_json::Map::new();
        update_attributes(&db, &uuid, empty_attrs).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(device.attributes.hostname.as_ref().unwrap(), "test-host");
    }

    #[tokio::test]
    async fn test_get_network_interfaces_empty() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x30);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_get_network_interfaces_single() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x31);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: None,
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        let retrieved = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].interface_name, "eth0");
        assert_eq!(retrieved[0].mac_address, "aa:bb:cc:dd:ee:01");
        assert_eq!(retrieved[0].ip_address, Some("10.0.0.100".to_string()));
    }

    #[tokio::test]
    async fn test_get_network_interfaces_multiple() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x32);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth2".to_string(),
                mac_address: "aa:bb:cc:dd:ee:03".to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        let retrieved = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].interface_name, "eth0");
        assert_eq!(retrieved[1].interface_name, "eth1");
        assert_eq!(retrieved[2].interface_name, "eth2");
    }

    #[tokio::test]
    async fn test_set_network_interfaces_overwrites() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x33);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let initial = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: None,
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];
        set_network_interfaces(&db, &uuid, &initial).await.unwrap();

        let updated = vec![
            NetworkInterface {
                interface_name: "ens0".to_string(),
                mac_address: "11:22:33:44:55:66".to_string(),
                ip_address: Some("192.168.1.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "ens1".to_string(),
                mac_address: "11:22:33:44:55:67".to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &updated).await.unwrap();

        let retrieved = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].interface_name, "ens0");
        assert_eq!(retrieved[1].interface_name, "ens1");
    }

    #[tokio::test]
    async fn test_find_device_by_mac_legacy_field() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x34);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_mac_address(&db, &uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let found = find_device_by_mac(&db, "aa:bb:cc:dd:ee:ff").await.unwrap();
        assert_eq!(found, Some(uuid));

        let not_found = find_device_by_mac(&db, "00:00:00:00:00:00").await.unwrap();
        assert_eq!(not_found, None);
    }

    #[tokio::test]
    async fn test_find_device_by_mac_in_interfaces_array() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x35);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        let found1 = find_device_by_mac(&db, "aa:bb:cc:dd:ee:01").await.unwrap();
        assert_eq!(found1, Some(uuid));

        let found2 = find_device_by_mac(&db, "aa:bb:cc:dd:ee:02").await.unwrap();
        assert_eq!(found2, Some(uuid));

        let not_found = find_device_by_mac(&db, "00:00:00:00:00:00").await.unwrap();
        assert_eq!(not_found, None);
    }

    #[tokio::test]
    async fn test_find_device_by_any_mac() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x36);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_mac_address(&db, &uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        // find_device_by_mac searches both legacy field and interfaces
        let found_legacy = find_device_by_mac(&db, "aa:bb:cc:dd:ee:ff").await.unwrap();
        assert_eq!(found_legacy, Some(uuid));

        let found_iface = find_device_by_mac(&db, "aa:bb:cc:dd:ee:02").await.unwrap();
        assert_eq!(found_iface, Some(uuid));
    }

    #[tokio::test]
    async fn test_set_mac_address_legacy_only() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x37);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_mac_address(&db, &uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.mac_address.as_ref().unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_set_mac_address_updates_first_interface() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x38);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        set_mac_address(&db, &uuid, "11:22:33:44:55:66")
            .await
            .unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.mac_address.as_ref().unwrap(),
            "11:22:33:44:55:66"
        );

        let updated_interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(updated_interfaces[0].mac_address, "11:22:33:44:55:66");
        assert_eq!(updated_interfaces[1].mac_address, "aa:bb:cc:dd:ee:02");
    }

    #[tokio::test]
    async fn test_set_ip_address_creates_interface_when_missing() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x39);
        let mac = "aa:bb:cc:dd:ee:ff";

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_ip_address(&db, &uuid, "10.0.0.100", mac).await.unwrap();

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, mac);
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.100".to_string()));

        let device = get_device(&db, &uuid).await.unwrap();
        assert!(device.attributes.static_ip.is_none());
    }

    #[tokio::test]
    async fn test_set_ip_address_updates_by_mac() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x40);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];
        set_network_interfaces(&db, &uuid, &interfaces)
            .await
            .unwrap();

        set_ip_address(&db, &uuid, "192.168.1.50", "aa:bb:cc:dd:ee:02")
            .await
            .unwrap();

        let updated_interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(
            updated_interfaces[1].ip_address,
            Some("192.168.1.50".to_string())
        );
        assert_eq!(
            updated_interfaces[0].ip_address,
            Some("10.0.0.100".to_string())
        );

        let device = get_device(&db, &uuid).await.unwrap();
        assert!(device.attributes.static_ip.is_none());
    }

    #[tokio::test]
    async fn test_backward_compatibility_legacy_device() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x41);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        set_mac_address(&db, &uuid, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();
        set_ip_address(&db, &uuid, "10.0.0.100", "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device.attributes.mac_address.as_ref().unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
        assert!(device.attributes.static_ip.is_none());

        let found = find_device_by_mac(&db, "aa:bb:cc:dd:ee:ff").await.unwrap();
        assert_eq!(found, Some(uuid));

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(interfaces[0].ip_address, Some("10.0.0.100".to_string()));
    }

    #[tokio::test]
    async fn test_set_ip_address_for_bmc() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x42);
        let bmc_mac = "aa:bb:cc:dd:ee:aa";

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        db.execute(
            r#"UPDATE devices SET attributes = json_set(attributes, '$.bmc',
               json('{"mac_address":"aa:bb:cc:dd:ee:aa","ip_address":null,"ip_address_source":"Unknown"}')
            ) WHERE uuid = ?1"#,
            (uuid,),
        )
        .await
        .unwrap();

        set_ip_address(&db, &uuid, "10.0.1.50", bmc_mac)
            .await
            .unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        let bmc = device.attributes.bmc.as_ref().unwrap();
        assert_eq!(bmc.ip_address.as_ref().unwrap(), "10.0.1.50");

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_get_network_interfaces_invalid_json() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x43);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        db.execute(
            "UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces', 'invalid') WHERE uuid = ?1",
            (uuid,),
        )
        .await
        .unwrap();

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 0);
    }

    #[tokio::test]
    async fn test_network_interface_disabled_fields_serialization() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x44);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let interface = NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:01".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(1),
            speed_mbps: None,
            disabled: true,
            warning_label: Some("Duplicate MAC on network main".to_string()),
        };
        set_network_interfaces(&db, &uuid, std::slice::from_ref(&interface))
            .await
            .unwrap();

        let retrieved = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].network_id, Some(1));
        assert!(retrieved[0].disabled);
        assert_eq!(
            retrieved[0].warning_label,
            Some("Duplicate MAC on network main".to_string())
        );
    }

    #[tokio::test]
    async fn test_network_interface_backward_compatibility() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x45);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        db.execute(
            r#"UPDATE devices SET attributes = json_set(attributes, '$.network_interfaces',
               json('[{"interface_name":"eth0","mac_address":"aa:bb:cc:dd:ee:01","ip_address":"10.0.0.100"}]')
            ) WHERE uuid = ?1"#,
            (uuid,),
        )
        .await
        .unwrap();

        let interfaces = get_network_interfaces(&db, &uuid).await.unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].network_id, None);
        assert!(!interfaces[0].disabled);
        assert_eq!(interfaces[0].warning_label, None);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_network_no_duplicates() {
        let db = setup_db(test_database_path!()).await;
        let uuid1 = test_uuid(0x46);
        let uuid2 = test_uuid(0x47);

        register_device(&db, &uuid1, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid2, Architecture::X86_64)
            .await
            .unwrap();

        set_network_interfaces(
            &db,
            &uuid1,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(1),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid2,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: Some(1),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();

        let duplicates = find_duplicate_macs_on_network(&db, "aa:bb:cc:dd:ee:01", 1, &uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_network_finds_duplicate() {
        let db = setup_db(test_database_path!()).await;
        let uuid1 = test_uuid(0x48);
        let uuid2 = test_uuid(0x49);

        register_device(&db, &uuid1, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid2, Architecture::X86_64)
            .await
            .unwrap();

        let mac = "aa:bb:cc:dd:ee:99";
        let network_id = 1i64;

        set_network_interfaces(
            &db,
            &uuid1,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid2,
            &[NetworkInterface {
                interface_name: "ens0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();

        let duplicates = find_duplicate_macs_on_network(&db, mac, network_id, &uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].0, uuid2);
        assert_eq!(duplicates[0].1, "ens0");

        let duplicates = find_duplicate_macs_on_network(&db, mac, network_id, &uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].0, uuid1);
        assert_eq!(duplicates[0].1, "eth0");
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_on_different_networks() {
        let db = setup_db(test_database_path!()).await;
        let uuid1 = test_uuid(0x4A);
        let uuid2 = test_uuid(0x4B);

        register_device(&db, &uuid1, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid2, Architecture::X86_64)
            .await
            .unwrap();

        let mac = "aa:bb:cc:dd:ee:88";
        set_network_interfaces(
            &db,
            &uuid1,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(1),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid2,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("192.168.1.100".to_string()),
                network_id: Some(2),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();

        let duplicates = find_duplicate_macs_on_network(&db, mac, 1i64, &uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);

        let duplicates = find_duplicate_macs_on_network(&db, mac, 2i64, &uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_multiple_duplicates() {
        let db = setup_db(test_database_path!()).await;
        let uuid1 = test_uuid(0x51);
        let uuid2 = test_uuid(0x52);
        let uuid3 = test_uuid(0x53);

        register_device(&db, &uuid1, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid2, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid3, Architecture::X86_64)
            .await
            .unwrap();

        let mac = "aa:bb:cc:dd:ee:77";
        let network_id = 1i64;

        set_network_interfaces(
            &db,
            &uuid1,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid2,
            &[NetworkInterface {
                interface_name: "ens0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.101".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid3,
            &[NetworkInterface {
                interface_name: "enp0s3".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.102".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();

        let mut duplicates = find_duplicate_macs_on_network(&db, mac, network_id, &uuid1)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 2);

        duplicates.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(duplicates[0].0, uuid2);
        assert_eq!(duplicates[0].1, "ens0");
        assert_eq!(duplicates[1].0, uuid3);
        assert_eq!(duplicates[1].1, "enp0s3");
    }

    #[tokio::test]
    async fn test_find_duplicate_macs_no_network_id() {
        let db = setup_db(test_database_path!()).await;
        let uuid1 = test_uuid(0x54);
        let uuid2 = test_uuid(0x55);

        register_device(&db, &uuid1, Architecture::X86_64)
            .await
            .unwrap();
        register_device(&db, &uuid2, Architecture::X86_64)
            .await
            .unwrap();

        let mac = "aa:bb:cc:dd:ee:66";
        let network_id = 1i64;

        set_network_interfaces(
            &db,
            &uuid1,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();
        set_network_interfaces(
            &db,
            &uuid2,
            &[NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac.to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                network_id: Some(network_id),
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            }],
        )
        .await
        .unwrap();

        let duplicates = find_duplicate_macs_on_network(&db, mac, network_id, &uuid2)
            .await
            .unwrap();
        assert_eq!(duplicates.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_pending_device() {
        let db = setup_db(test_database_path!()).await;
        let network_id = create_test_network(&db).await;
        let mac = "aa:bb:cc:dd:ee:99";

        let pending_id = create_pending_device(&db, mac, network_id).await.unwrap();

        let pending_devices = get_pending_devices(&db).await.unwrap();
        assert_eq!(pending_devices.len(), 1);
        assert_eq!(pending_devices[0].id, pending_id);
        assert_eq!(pending_devices[0].mac_address, mac);

        delete_pending_device(&db, pending_id).await.unwrap();

        let pending_devices = get_pending_devices(&db).await.unwrap();
        assert_eq!(pending_devices.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_pending_device() {
        let db = setup_db(test_database_path!()).await;

        let result = delete_pending_device(&db, 999).await;
        assert!(result.is_ok());
    }

    // ===== Failure-mode tests =====

    /// get_device with a UUID that does not exist must return an error.
    #[tokio::test]
    async fn test_get_device_not_found() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF0);

        let result = get_device(&db, &uuid).await;
        assert!(
            result.is_err(),
            "get_device must return Err for unknown UUID"
        );
    }

    /// register_device with a UUID that is already present must return an error
    /// because of the PRIMARY KEY constraint on the devices table.
    #[tokio::test]
    async fn test_register_device_duplicate_uuid() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF1);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        let result = register_device(&db, &uuid, Architecture::X86_64).await;
        assert!(
            result.is_err(),
            "register_device must return Err for duplicate UUID"
        );
    }

    /// device_exists with a UUID that does not exist must return false (not an error).
    #[tokio::test]
    async fn test_device_exists_not_found() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF2);

        let exists = device_exists(&db, &uuid).await.unwrap();
        assert!(!exists, "device_exists must return false for unknown UUID");
    }

    /// find_device_by_mac with a MAC that matches no device must return None.
    #[tokio::test]
    async fn test_find_device_by_mac_not_found() {
        let db = setup_db(test_database_path!()).await;

        let result = find_device_by_mac(&db, "ff:ff:ff:ff:ff:ff").await.unwrap();
        assert_eq!(
            result, None,
            "find_device_by_mac must return None when MAC is absent"
        );
    }

    /// delete_device with a UUID that does not exist must succeed silently
    /// (DELETE with no matching rows is not an error in SQLite).
    #[tokio::test]
    async fn test_delete_device_nonexistent() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF3);

        let result = delete_device(&db, &uuid).await;
        assert!(
            result.is_ok(),
            "delete_device must return Ok for unknown UUID"
        );
    }

    /// update_attributes with a UUID that does not exist must return an error
    /// because it calls get_device internally, which fails for unknown UUIDs.
    #[tokio::test]
    async fn test_update_attributes_nonexistent() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF4);

        let attrs = serde_json::Map::new();
        let result = update_attributes(&db, &uuid, attrs).await;
        assert!(
            result.is_err(),
            "update_attributes must return Err for unknown UUID"
        );
    }

    /// complete_pending_device with a MAC that does not exist in pending_devices
    /// must return Ok — the UPDATE simply affects 0 rows, which is not an error.
    #[tokio::test]
    async fn test_complete_pending_device_nonexistent() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF5);

        let result = complete_pending_device(&db, "ff:00:00:00:00:00", &uuid).await;
        assert!(
            result.is_ok(),
            "complete_pending_device must return Ok when MAC is absent (0 rows updated)"
        );
    }

    /// assign_platform_to_device with a UUID that does not exist must return Ok —
    /// the UPDATE simply affects 0 rows, which is not an error.
    #[tokio::test]
    async fn test_assign_platform_nonexistent() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF6);

        let result = assign_platform_to_device(&db, &uuid, 999).await;
        assert!(
            result.is_ok(),
            "assign_platform_to_device must return Ok when device UUID is absent"
        );
    }

    /// assign_role_to_device with a UUID that does not exist must return Ok —
    /// the UPDATE simply affects 0 rows, which is not an error.
    #[tokio::test]
    async fn test_assign_role_nonexistent() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0xF7);

        let result = assign_role_to_device(&db, &uuid, 999).await;
        assert!(
            result.is_ok(),
            "assign_role_to_device must return Ok when device UUID is absent"
        );
    }

    #[tokio::test]
    async fn test_delete_device_with_pending() {
        let db = setup_db(test_database_path!()).await;
        let network_id = create_test_network(&db).await;
        let uuid = test_uuid(0x56);
        let mac = "aa:bb:cc:dd:ee:99";

        create_pending_device(&db, mac, network_id).await.unwrap();

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        complete_pending_device(&db, mac, &uuid).await.unwrap();

        delete_device(&db, &uuid).await.unwrap();

        let exists: rusqlite::Result<bool> = db
            .query_one(
                "SELECT 1 FROM pending_devices WHERE mac_address = ?1",
                (mac.to_string(),),
                |r| r.get(0),
            )
            .await
            .optional()
            .map(|op: Option<bool>| op.is_some());
        assert!(
            matches!(exists, Ok(false)),
            "Deleting a Device must delete pending records. {:?}",
            exists
        );
    }

    /// Override is preserved when the referenced disk path still exists in the new scan.
    #[tokio::test]
    async fn test_update_attributes_preserves_valid_override() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x57);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set an existing override pointing to a path we will include in the scan
        let path = "/dev/disk/by-path/pci-0000:00:1f.2-ata-1";
        let mut initial_attrs = serde_json::Map::new();
        let mut overrides = serde_json::Map::new();
        overrides.insert(
            "ROOT".to_string(),
            serde_json::Value::String(path.to_string()),
        );
        initial_attrs.insert(
            "disk_label_overrides".to_string(),
            serde_json::Value::Object(overrides),
        );
        update_attributes(&db, &uuid, initial_attrs).await.unwrap();

        // Submit a hardware scan that includes the same path
        let disks = serde_json::json!([{
            "name": "sda",
            "path": path,
            "size": 480,
            "disk_type": "ssd"
        }]);
        let mut scan_attrs = serde_json::Map::new();
        scan_attrs.insert("disks".to_string(), disks);
        update_attributes(&db, &uuid, scan_attrs).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert_eq!(
            device
                .attributes
                .disk_label_overrides
                .get("ROOT")
                .map(|s| s.as_str()),
            Some(path),
            "override for ROOT should be preserved when path still present"
        );

        // No warning should have been created
        let device_id: i64 = db
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid,), |r| {
                r.get(0)
            })
            .await
            .unwrap();
        let warning_count: i64 = db
            .query_one(
                "SELECT COUNT(*) FROM device_warnings WHERE device_id = ?1",
                (device_id,),
                |r| r.get(0),
            )
            .await
            .unwrap();
        assert_eq!(
            warning_count, 0,
            "no warnings should exist for valid override"
        );
    }

    /// Override is dropped and a warning is created when the referenced path disappears.
    #[tokio::test]
    async fn test_update_attributes_drops_stale_override_and_creates_warning() {
        let db = setup_db(test_database_path!()).await;
        let uuid = test_uuid(0x58);

        register_device(&db, &uuid, Architecture::X86_64)
            .await
            .unwrap();

        // Set an override pointing to a path that will NOT appear in the new scan
        let stale_path = "/dev/disk/by-path/pci-0000:00:1f.2-ata-old";
        let mut initial_attrs = serde_json::Map::new();
        let mut overrides = serde_json::Map::new();
        overrides.insert(
            "ROOT".to_string(),
            serde_json::Value::String(stale_path.to_string()),
        );
        initial_attrs.insert(
            "disk_label_overrides".to_string(),
            serde_json::Value::Object(overrides),
        );
        update_attributes(&db, &uuid, initial_attrs).await.unwrap();

        // Submit a hardware scan with a *different* path
        let new_path = "/dev/disk/by-path/pci-0000:00:1f.2-ata-new";
        let disks = serde_json::json!([{
            "name": "sda",
            "path": new_path,
            "size": 480,
            "disk_type": "ssd"
        }]);
        let mut scan_attrs = serde_json::Map::new();
        scan_attrs.insert("disks".to_string(), disks);
        update_attributes(&db, &uuid, scan_attrs).await.unwrap();

        let device = get_device(&db, &uuid).await.unwrap();
        assert!(
            device.attributes.disk_label_overrides.get("ROOT").is_none(),
            "stale override for ROOT should have been removed"
        );

        // A LABEL_OVERRIDE_DROPPED warning must have been created
        let device_id: i64 = db
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid,), |r| {
                r.get(0)
            })
            .await
            .unwrap();
        let warnings = crate::device_warnings::list_warnings(&db, device_id)
            .await
            .unwrap();
        assert_eq!(warnings.len(), 1, "exactly one warning should exist");
        assert_eq!(warnings[0].code, "LABEL_OVERRIDE_DROPPED");
        assert!(
            warnings[0].message.contains("ROOT"),
            "warning message should mention the label"
        );
        assert!(
            warnings[0].message.contains(stale_path),
            "warning message should mention the stale path"
        );
    }

    /// Confirms that `collect_stale_overrides` correctly identifies stale entries.
    #[test]
    fn test_collect_stale_overrides_detects_missing_paths() {
        use common::device_attributes::DiskInfo;

        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "ROOT".to_string(),
            "/dev/disk/by-path/good-path".to_string(),
        );
        overrides.insert(
            "DATA1".to_string(),
            "/dev/disk/by-path/gone-path".to_string(),
        );

        let attrs = DeviceAttributes {
            disks: vec![DiskInfo {
                name: "sda".to_string(),
                path: Some("/dev/disk/by-path/good-path".to_string()),
                size: Some(480),
                disk_type: None,
                model: None,
                serial: None,
                vendor: None,
                uuid: None,
            }],
            disk_label_overrides: overrides,
            ..Default::default()
        };

        let stale = collect_stale_overrides(&attrs);
        assert_eq!(stale.len(), 1, "only DATA1 should be stale");
        assert_eq!(stale[0].0, "DATA1");
        assert_eq!(stale[0].1, "/dev/disk/by-path/gone-path");
    }

    /// Confirms that `collect_stale_overrides` returns an empty list when no overrides exist.
    #[test]
    fn test_collect_stale_overrides_empty_when_no_overrides() {
        let attrs = DeviceAttributes {
            disks: vec![],
            ..Default::default()
        };
        assert!(collect_stale_overrides(&attrs).is_empty());
    }
}
