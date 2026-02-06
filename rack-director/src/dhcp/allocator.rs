use anyhow::Result;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use uuid::Uuid;

use super::store::DhcpStore;

#[derive(Clone)]
pub struct IpAllocator {
    store: DhcpStore,
}

impl IpAllocator {
    pub fn new(store: DhcpStore) -> Self {
        Self { store }
    }

    /// Allocate IP for a known device (MAC -> UUID mapping exists) within a specific network
    pub async fn allocate_for_device_in_network(
        &self,
        mac: &str,
        uuid: &Uuid,
        network_id: i64,
    ) -> Result<Ipv4Addr> {
        // 1. Check static reservation in this network
        if let Some(reservation) = self.store.get_static_reservation(network_id, mac).await? {
            log::debug!(
                "Device {} has static reservation {} in network {}",
                uuid,
                reservation.ip_address,
                network_id
            );
            return Ok(reservation.ip_address.parse()?);
        }

        // 2. Check existing lease in this network
        if let Some(lease) = self.store.get_lease_by_mac(mac).await?
            && !lease.is_expired()
            && lease.network_id == Some(network_id)
        {
            log::debug!(
                "Reusing existing lease for MAC {} in network {}: {}",
                mac,
                network_id,
                lease.ip_address
            );
            return Ok(lease.ip_address.parse()?);
        }

        // 3. Allocate from pools in this network
        self.allocate_from_pools(network_id, mac).await
    }

    /// Allocate IP for unknown device (no UUID mapping) within a specific network
    pub async fn allocate_for_mac_in_network(
        &self,
        mac: &str,
        network_id: i64,
    ) -> Result<Ipv4Addr> {
        // 1. Check static reservation in this network
        if let Some(reservation) = self.store.get_static_reservation(network_id, mac).await? {
            log::debug!(
                "MAC {} has static reservation {} in network {}",
                mac,
                reservation.ip_address,
                network_id
            );
            return Ok(reservation.ip_address.parse()?);
        }

        // 2. Check existing lease in this network
        if let Some(lease) = self.store.get_lease_by_mac(mac).await?
            && !lease.is_expired()
            && lease.network_id == Some(network_id)
        {
            log::debug!(
                "Reusing existing lease for MAC {} in network {}: {}",
                mac,
                network_id,
                lease.ip_address
            );
            return Ok(lease.ip_address.parse()?);
        }

        // 3. Allocate from pools in this network
        self.allocate_from_pools(network_id, mac).await
    }

    /// Allocate from pools within a network (try each pool until success)
    async fn allocate_from_pools(&self, network_id: i64, mac: &str) -> Result<Ipv4Addr> {
        let pools = self.store.list_pools_for_network(network_id).await?;

        if pools.is_empty() {
            return Err(anyhow::anyhow!(
                "No pools configured for network {}",
                network_id
            ));
        }

        // Get all active IPs in this network
        let active_ips: HashSet<Ipv4Addr> = self
            .store
            .get_leases_by_network(network_id)
            .await?
            .into_iter()
            .filter(|l| !l.is_expired())
            .filter_map(|l| l.ip_address.parse().ok())
            .collect();

        // Get all statically reserved IPs in this network
        let reserved_ips: HashSet<Ipv4Addr> = self
            .store
            .list_static_reservations(network_id)
            .await?
            .into_iter()
            .filter_map(|r| r.ip_address.parse().ok())
            .collect();

        // Try each pool until allocation succeeds
        for pool in pools {
            let range = parse_ip_range(&pool.range_start, &pool.range_end)?;

            for ip in range {
                if !active_ips.contains(&ip) && !reserved_ips.contains(&ip) {
                    log::info!(
                        "Allocated {} from pool '{}' (network {}) for MAC {}",
                        ip,
                        pool.name,
                        network_id,
                        mac
                    );
                    return Ok(ip);
                }
            }
        }

        Err(anyhow::anyhow!(
            "All pools exhausted for network {}",
            network_id
        ))
    }
}

/// Parse IP range from start and end addresses
fn parse_ip_range(start: &str, end: &str) -> Result<impl Iterator<Item = Ipv4Addr>> {
    let start_ip: Ipv4Addr = start.parse()?;
    let end_ip: Ipv4Addr = end.parse()?;

    let start_u32: u32 = start_ip.into();
    let end_u32: u32 = end_ip.into();

    Ok((start_u32..=end_u32).map(Ipv4Addr::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::dhcp::store::LeaseState;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_allocator() -> (IpAllocator, i64, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let store = DhcpStore::new(db.clone());

        // Create test network (migration 12 removed the default network)
        let network = store
            .create_network(
                "Test Network",
                "10.0.0.0/24",
                "10.0.0.1",
                &["8.8.8.8".to_string(), "8.8.4.4".to_string()],
                86400,
                None,
                false,
            )
            .await
            .unwrap();

        // Create test pool
        store
            .create_pool(network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        (IpAllocator::new(store), network.id, temp_dir)
    }

    #[tokio::test]
    async fn test_allocate_for_mac_in_network() {
        let (allocator, network_id, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // Allocate IP in test network
        let ip = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), "10.0.0.100"); // First IP in test range
    }

    #[tokio::test]
    async fn test_allocate_reuses_existing_lease() {
        let (allocator, network_id, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // First allocation
        let ip1 = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();

        // Create lease
        allocator
            .store
            .create_or_update_lease_with_network(
                mac,
                &ip1,
                None,
                LeaseState::Active,
                3600,
                network_id,
            )
            .await
            .unwrap();

        // Second allocation should return same IP
        let ip2 = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip1, ip2);
    }

    #[tokio::test]
    async fn test_allocate_different_ips_for_different_macs() {
        let (allocator, network_id, _temp_dir) = create_test_allocator().await;
        let mac1 = "aa:bb:cc:dd:ee:ff";
        let mac2 = "11:22:33:44:55:66";

        let ip1 = allocator
            .allocate_for_mac_in_network(mac1, network_id)
            .await
            .unwrap();
        allocator
            .store
            .create_or_update_lease_with_network(
                mac1,
                &ip1,
                None,
                LeaseState::Active,
                3600,
                network_id,
            )
            .await
            .unwrap();

        let ip2 = allocator
            .allocate_for_mac_in_network(mac2, network_id)
            .await
            .unwrap();

        assert_ne!(ip1, ip2);
        assert_eq!(ip1.to_string(), "10.0.0.100");
        assert_eq!(ip2.to_string(), "10.0.0.101");
    }

    #[tokio::test]
    async fn test_static_reservation_takes_priority() {
        let (allocator, network_id, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";
        let static_ip = "10.0.0.50";

        // Create static reservation
        allocator
            .store
            .create_static_reservation(network_id, mac, static_ip, None)
            .await
            .unwrap();

        // Allocation should return the static IP
        let ip = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), static_ip);
    }

    #[tokio::test]
    async fn test_static_reservation_overrides_existing_lease() {
        let (allocator, network_id, _temp_dir) = create_test_allocator().await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // First, allocate IP from pool
        let ip1 = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip1.to_string(), "10.0.0.100"); // First IP in pool

        // Create an active lease for this IP
        allocator
            .store
            .create_or_update_lease_with_network(
                mac,
                &ip1,
                None,
                LeaseState::Active,
                3600,
                network_id,
            )
            .await
            .unwrap();

        // Admin creates a static reservation for a different IP
        let static_ip = "10.0.0.50";
        allocator
            .store
            .create_static_reservation(network_id, mac, static_ip, None)
            .await
            .unwrap();

        // Next allocation should return the static IP, not the existing lease IP
        let ip2 = allocator
            .allocate_for_mac_in_network(mac, network_id)
            .await
            .unwrap();
        assert_eq!(
            ip2.to_string(),
            static_ip,
            "Static reservation should override existing lease"
        );
    }

    #[tokio::test]
    async fn test_parse_ip_range() {
        let range: Vec<Ipv4Addr> = parse_ip_range("10.0.0.1", "10.0.0.5").unwrap().collect();

        assert_eq!(range.len(), 5);
        assert_eq!(range[0].to_string(), "10.0.0.1");
        assert_eq!(range[4].to_string(), "10.0.0.5");
    }
}
