use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::director::Director;

/// Pre-resolved device context for DHCP handling.
pub struct DeviceContext {
    pub device_uuid: Option<Uuid>,
    pub is_disabled: bool,
    pub disable_reason: Option<String>,
    pub is_pending: bool,
}

/// Trait for resolving device information from a MAC address and optional GUID.
#[async_trait]
pub trait DeviceResolver: Send + Sync {
    /// Resolve device context from a MAC address and optional GUID.
    ///
    /// Resolution priority:
    /// 1. If GUID is provided and matches a known device, use that device
    /// 2. Otherwise, fall back to MAC-based resolution
    async fn resolve(&self, mac: &str, guid: Option<&Uuid>) -> Result<DeviceContext>;

    /// Notify that a lease has been activated for a device.
    async fn on_lease_activated(&self, uuid: &Uuid, ip: &str, mac: &str) -> Result<()>;
}

/// DeviceResolver implementation backed by the Director service.
pub struct DirectorDeviceResolver {
    director: Director,
}

impl DirectorDeviceResolver {
    pub fn new(director: Director) -> Self {
        Self { director }
    }
}

#[async_trait]
impl DeviceResolver for DirectorDeviceResolver {
    async fn resolve(&self, mac: &str, guid: Option<&Uuid>) -> Result<DeviceContext> {
        // Try GUID-based resolution first if GUID is provided
        let mut device_uuid = if let Some(guid) = guid {
            if self.director.device_exists(guid).await? {
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
            device_uuid = self.director.find_device_by_mac(mac).await?;
            if device_uuid.is_none()
                && let Some(bmc_uuid) = self.director.find_device_by_bmc_mac(mac).await?
            {
                log::info!("MAC {} is a BMC for device {}", mac, bmc_uuid);
                device_uuid = Some(bmc_uuid);
            }
        }

        // Check if interface is disabled
        let (is_disabled, disable_reason) = if let Some(uuid) = &device_uuid {
            let interfaces = self.director.get_network_interfaces(uuid).await?;
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

        // Check if pending device
        let is_pending = self
            .director
            .find_pending_device_by_mac(mac)
            .await?
            .is_some();

        Ok(DeviceContext {
            device_uuid,
            is_disabled,
            disable_reason,
            is_pending,
        })
    }

    async fn on_lease_activated(&self, uuid: &Uuid, ip: &str, mac: &str) -> Result<()> {
        self.director.set_device_ip_address(uuid, ip, mac).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_resolver() -> DirectorDeviceResolver {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let director = Director::new(db.clone());
        DirectorDeviceResolver::new(director)
    }

    #[tokio::test]
    async fn test_resolve_unknown_mac() {
        let resolver = create_test_resolver().await;
        let ctx = resolver.resolve("aa:bb:cc:dd:ee:ff", None).await.unwrap();
        assert!(ctx.device_uuid.is_none());
        assert!(!ctx.is_disabled);
        assert!(ctx.disable_reason.is_none());
        assert!(!ctx.is_pending);
    }

    #[tokio::test]
    async fn test_resolve_returns_not_disabled_for_unknown() {
        let resolver = create_test_resolver().await;
        let ctx = resolver.resolve("11:22:33:44:55:66", None).await.unwrap();
        assert!(!ctx.is_disabled);
    }

    #[tokio::test]
    async fn test_resolve_returns_not_pending_for_unknown() {
        let resolver = create_test_resolver().await;
        let ctx = resolver.resolve("11:22:33:44:55:66", None).await.unwrap();
        assert!(!ctx.is_pending);
    }

    #[tokio::test]
    async fn test_resolve_with_guid_no_match() {
        let resolver = create_test_resolver().await;

        // Use a GUID that doesn't exist in the database
        let non_existent_guid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        // Resolve with non-matching GUID should return None for device_uuid
        let ctx = resolver
            .resolve("aa:bb:cc:dd:ee:ff", Some(&non_existent_guid))
            .await
            .unwrap();
        assert_eq!(ctx.device_uuid, None);
    }

    #[tokio::test]
    async fn test_resolve_without_guid() {
        let resolver = create_test_resolver().await;

        // Resolve without GUID should use MAC-based resolution (returns None for unknown MAC)
        let ctx = resolver.resolve("aa:bb:cc:dd:ee:ff", None).await.unwrap();
        assert_eq!(ctx.device_uuid, None);
    }
}
