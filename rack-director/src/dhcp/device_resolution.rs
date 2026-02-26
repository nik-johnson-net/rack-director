use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::database::Connection;
use crate::director::Director;

/// Pre-resolved device context for DHCP handling.
pub struct DeviceContext {
    pub device_uuid: Option<Uuid>,
    pub is_disabled: bool,
    pub disable_reason: Option<String>,
}

/// Trait for resolving device information from a MAC address and optional GUID.
#[async_trait]
pub trait DeviceResolver: Send + Sync {
    /// Resolve device context from a MAC address and optional GUID.
    ///
    /// Resolution priority:
    /// 1. If GUID is provided and matches a known device, use that device
    /// 2. Otherwise, fall back to MAC-based resolution
    async fn resolve(
        &self,
        conn: &Connection,
        mac: &str,
        guid: Option<&Uuid>,
    ) -> Result<DeviceContext>;

    /// Notify that a lease has been activated for a device.
    async fn on_lease_activated(
        &self,
        conn: &Connection,
        uuid: &Uuid,
        ip: &str,
        mac: &str,
    ) -> Result<()>;
}

/// Stateless DeviceResolver implementation backed by the Director service.
///
/// Each call constructs a short-lived `Director` from the provided connection, so no
/// connection is stored here. This keeps `DirectorDeviceResolver` cheaply cloneable
/// and `Send + Sync`.
pub struct DirectorDeviceResolver;

impl DirectorDeviceResolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DirectorDeviceResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeviceResolver for DirectorDeviceResolver {
    async fn resolve(
        &self,
        conn: &Connection,
        mac: &str,
        guid: Option<&Uuid>,
    ) -> Result<DeviceContext> {
        let director = Director::new(conn);

        // Try GUID-based resolution first if GUID is provided
        let mut device_uuid = if let Some(guid) = guid {
            if director.device_exists(guid).await? {
                log::debug!("Resolved device {} via GUID", guid);
                Some(*guid)
            } else {
                None
            }
        } else {
            None
        };

        // Fall back to MAC-based resolution if GUID didn't match
        if device_uuid.is_none() {
            device_uuid = director.find_device_by_mac(mac).await?;
            if device_uuid.is_none()
                && let Some(bmc_uuid) = director.find_device_by_bmc_mac(mac).await?
            {
                log::info!("MAC {} is a BMC for device {}", mac, bmc_uuid);
                device_uuid = Some(bmc_uuid);
            }
        }

        // Check if interface is disabled
        let (is_disabled, disable_reason) = if let Some(uuid) = &device_uuid {
            let interfaces = director.get_network_interfaces(uuid).await?;
            if let Some(iface) = interfaces.iter().find(|i| i.mac_address == mac) {
                if iface.disabled {
                    (true, iface.warning_label.clone())
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        Ok(DeviceContext {
            device_uuid,
            is_disabled,
            disable_reason,
        })
    }

    async fn on_lease_activated(
        &self,
        conn: &Connection,
        uuid: &Uuid,
        ip: &str,
        mac: &str,
    ) -> Result<()> {
        let director = Director::new(conn);
        director.set_device_ip_address(uuid, ip, mac).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{self, DatabaseConnectionFactory};
    use crate::test_connection_factory;

    async fn create_test_db(factory: DatabaseConnectionFactory) -> database::Connection {
        database::run_migrations(&factory).await.unwrap()
    }

    #[tokio::test]
    async fn test_resolve_unknown_mac() {
        let conn = create_test_db(test_connection_factory!()).await;
        let resolver = DirectorDeviceResolver::new();
        let ctx = resolver
            .resolve(&conn, "aa:bb:cc:dd:ee:ff", None)
            .await
            .unwrap();
        assert!(ctx.device_uuid.is_none());
        assert!(!ctx.is_disabled);
        assert!(ctx.disable_reason.is_none());
    }

    #[tokio::test]
    async fn test_resolve_returns_not_disabled_for_unknown() {
        let conn = create_test_db(test_connection_factory!()).await;
        let resolver = DirectorDeviceResolver::new();
        let ctx = resolver
            .resolve(&conn, "11:22:33:44:55:66", None)
            .await
            .unwrap();
        assert!(!ctx.is_disabled);
    }

    #[tokio::test]
    async fn test_resolve_returns_not_pending_for_unknown() {
        let conn = create_test_db(test_connection_factory!()).await;
        let resolver = DirectorDeviceResolver::new();
        let _ctx = resolver
            .resolve(&conn, "11:22:33:44:55:66", None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_resolve_with_guid_no_match() {
        let conn = create_test_db(test_connection_factory!()).await;
        let resolver = DirectorDeviceResolver::new();

        // Use a GUID that doesn't exist in the database
        let non_existent_guid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        // Resolve with non-matching GUID should return None for device_uuid
        let ctx = resolver
            .resolve(&conn, "aa:bb:cc:dd:ee:ff", Some(&non_existent_guid))
            .await
            .unwrap();
        assert_eq!(ctx.device_uuid, None);
    }

    #[tokio::test]
    async fn test_resolve_without_guid() {
        let conn = create_test_db(test_connection_factory!()).await;
        let resolver = DirectorDeviceResolver::new();

        // Resolve without GUID should use MAC-based resolution (returns None for unknown MAC)
        let ctx = resolver
            .resolve(&conn, "aa:bb:cc:dd:ee:ff", None)
            .await
            .unwrap();
        assert_eq!(ctx.device_uuid, None);
    }
}
