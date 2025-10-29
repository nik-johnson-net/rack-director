use anyhow::Result;
use std::collections::HashSet;
use std::net::Ipv4Addr;

use crate::director::Director;

use super::store::{DhcpConfig, DhcpStore};

#[derive(Clone)]
pub struct IpAllocator {
    store: DhcpStore,
    director: Director,
    config: DhcpConfig,
}

impl IpAllocator {
    pub fn new(store: DhcpStore, director: Director, config: DhcpConfig) -> Self {
        Self {
            store,
            director,
            config,
        }
    }

    /// Allocate IP for a known device (MAC -> UUID mapping exists)
    pub async fn allocate_for_device(&self, mac: &str, uuid: &str) -> Result<Ipv4Addr> {
        // Check if device has static IP in attributes
        if let Some(static_ip) = self.director.get_device_static_ip(uuid).await? {
            log::debug!("Device {} has static IP {}", uuid, static_ip);
            return Ok(static_ip);
        }

        // Check existing lease
        if let Some(lease) = self.store.get_lease_by_mac(mac).await?
            && !lease.is_expired()
        {
            log::debug!(
                "Reusing existing lease for MAC {}: {}",
                mac,
                lease.ip_address
            );
            return Ok(lease.ip_address.parse()?);
        }

        // Allocate from pool
        self.allocate_from_pool(mac).await
    }

    /// Allocate IP for unknown device (no UUID mapping)
    pub async fn allocate_for_mac(&self, mac: &str) -> Result<Ipv4Addr> {
        // Check existing lease
        if let Some(lease) = self.store.get_lease_by_mac(mac).await?
            && !lease.is_expired()
        {
            log::debug!(
                "Reusing existing lease for MAC {}: {}",
                mac,
                lease.ip_address
            );
            return Ok(lease.ip_address.parse()?);
        }

        // Allocate from pool
        self.allocate_from_pool(mac).await
    }

    async fn allocate_from_pool(&self, mac: &str) -> Result<Ipv4Addr> {
        let range = self.parse_range()?;

        // Get all active leases
        let active_ips: HashSet<Ipv4Addr> = self
            .store
            .get_active_leases()
            .await?
            .into_iter()
            .filter_map(|l| l.ip_address.parse().ok())
            .collect();

        // Find first available IP
        for ip in range {
            if !active_ips.contains(&ip) {
                log::info!("Allocated IP {} for MAC {}", ip, mac);
                return Ok(ip);
            }
        }

        Err(anyhow::anyhow!("DHCP pool exhausted"))
    }

    fn parse_range(&self) -> Result<impl Iterator<Item = Ipv4Addr>> {
        let start: Ipv4Addr = self.config.range_start.parse()?;
        let end: Ipv4Addr = self.config.range_end.parse()?;

        let start_u32: u32 = start.into();
        let end_u32: u32 = end.into();

        Ok((start_u32..=end_u32).map(Ipv4Addr::from))
    }

    pub fn config(&self) -> &DhcpConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::dhcp::store::LeaseState;
    use crate::director::Director;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_allocator() -> (IpAllocator, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let store = DhcpStore::new(db.clone());
        let director = Director::new(db);
        let config = store.load_config().await.unwrap();
        (IpAllocator::new(store, director, config), temp_dir)
    }

    #[tokio::test]
    async fn test_allocate_for_mac() {
        let (allocator, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";

        let ip = allocator.allocate_for_mac(mac).await.unwrap();
        assert_eq!(ip.to_string(), "10.0.0.100"); // First IP in default range
    }

    #[tokio::test]
    async fn test_allocate_reuses_existing_lease() {
        let (allocator, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // First allocation
        let ip1 = allocator.allocate_for_mac(mac).await.unwrap();

        // Create lease
        allocator
            .store
            .create_or_update_lease(mac, &ip1, None, LeaseState::Active, 3600)
            .await
            .unwrap();

        // Second allocation should return same IP
        let ip2 = allocator.allocate_for_mac(mac).await.unwrap();
        assert_eq!(ip1, ip2);
    }

    #[tokio::test]
    async fn test_allocate_different_ips_for_different_macs() {
        let (allocator, _temp_dir) = create_test_allocator().await;
        let mac1 = "aa:bb:cc:dd:ee:ff";
        let mac2 = "11:22:33:44:55:66";

        let ip1 = allocator.allocate_for_mac(mac1).await.unwrap();
        allocator
            .store
            .create_or_update_lease(mac1, &ip1, None, LeaseState::Active, 3600)
            .await
            .unwrap();

        let ip2 = allocator.allocate_for_mac(mac2).await.unwrap();

        assert_ne!(ip1, ip2);
        assert_eq!(ip1.to_string(), "10.0.0.100");
        assert_eq!(ip2.to_string(), "10.0.0.101");
    }
}
