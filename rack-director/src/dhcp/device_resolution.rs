use anyhow::Result;

use crate::director::Director;

/// Resolves a device UUID from a MAC address.
///
/// This function attempts to find a device by MAC address in two ways:
/// 1. First, checks if the MAC belongs to a device NIC (network interface)
/// 2. If not found, checks if the MAC belongs to a BMC (Baseboard Management Controller)
///
/// # Arguments
/// * `director` - The director service for device lookups
/// * `mac_str` - The MAC address to resolve (formatted string)
///
/// # Returns
/// * `Ok(Some(uuid))` - Device found, returns the UUID
/// * `Ok(None)` - Device not found
/// * `Err(_)` - Database or lookup error
pub async fn resolve_device_uuid(director: &Director, mac_str: &str) -> Result<Option<String>> {
    // Check if device exists in devices table by MAC (device NIC)
    let mut device_uuid = director.find_device_by_mac(mac_str).await?;

    // If not found, check if this MAC belongs to a BMC
    if device_uuid.is_none()
        && let Some(bmc_device_uuid) = director.find_device_by_bmc_mac(mac_str).await?
    {
        log::info!("MAC {} is a BMC for device {}", mac_str, bmc_device_uuid);
        device_uuid = Some(bmc_device_uuid);
    }

    Ok(device_uuid)
}

/// Checks if a network interface is disabled for the given device and MAC address.
///
/// An interface may be disabled due to duplicate MAC detection or other administrative
/// reasons. Disabled interfaces should not receive DHCP responses.
///
/// # Arguments
/// * `director` - The director service for interface lookups
/// * `device_uuid` - The device UUID to check
/// * `mac_str` - The MAC address of the interface
///
/// # Returns
/// * `Ok((true, Some(reason)))` - Interface is disabled with a reason
/// * `Ok((false, None))` - Interface is enabled
/// * `Err(_)` - Database or lookup error
pub async fn is_interface_disabled(
    director: &Director,
    device_uuid: &str,
    mac_str: &str,
) -> Result<(bool, Option<String>)> {
    let interfaces = director.get_network_interfaces(device_uuid).await?;

    if let Some(iface) = interfaces.iter().find(|i| i.mac_address == mac_str)
        && iface.disabled
    {
        return Ok((true, iface.warning_label.clone()));
    }

    Ok((false, None))
}

/// Checks if a MAC address corresponds to a pending device.
///
/// Pending devices are devices that have been detected but not yet fully registered
/// in the system. They may be allowed to boot depending on the autodiscover setting.
///
/// # Arguments
/// * `director` - The director service for pending device lookups
/// * `mac_str` - The MAC address to check
///
/// # Returns
/// * `Ok(true)` - MAC belongs to a pending device
/// * `Ok(false)` - MAC does not belong to a pending device
/// * `Err(_)` - Database or lookup error
pub async fn is_pending_device(director: &Director, mac_str: &str) -> Result<bool> {
    Ok(director
        .find_pending_device_by_mac(mac_str)
        .await?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::storage::MemoryImageStore;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_director() -> Director {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        Director::new(
            db.clone(),
            Arc::new(MemoryImageStore::new()),
            "http://localhost:8080",
        )
    }

    #[tokio::test]
    async fn test_resolve_device_uuid_not_found() {
        let director = create_test_director().await;
        let result = resolve_device_uuid(&director, "aa:bb:cc:dd:ee:ff").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_is_interface_disabled_no_device() {
        let director = create_test_director().await;
        // Non-existent device UUID should return Ok((false, None))
        let result = is_interface_disabled(&director, "non-existent-uuid", "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();
        assert_eq!(result, (false, None));
    }

    #[tokio::test]
    async fn test_is_pending_device_not_found() {
        let director = create_test_director().await;
        let result = is_pending_device(&director, "aa:bb:cc:dd:ee:ff")
            .await
            .unwrap();
        assert_eq!(result, false);
    }

    // Note: Full integration tests would require setting up devices properly
    // These tests verify the functions compile and handle the basic cases
}
