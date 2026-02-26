use anyhow::Result;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use uuid::Uuid;

use crate::database::Connection;

use super::store;

/// Allocate IP for a known device (MAC -> UUID mapping exists) within a specific network
pub async fn allocate_for_device_in_network(
    conn: &Connection,
    mac: &str,
    uuid: &Uuid,
    network_id: i64,
) -> Result<Ipv4Addr> {
    // 1. Check static reservation in this network
    if let Some(reservation) = store::get_static_reservation(conn, network_id, mac).await? {
        log::debug!(
            "Device {} has static reservation {} in network {}",
            uuid,
            reservation.ip_address,
            network_id
        );
        return Ok(reservation.ip_address.parse()?);
    }

    // 2. Check existing lease in this network
    if let Some(lease) = store::get_lease_by_mac(conn, mac).await?
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
    allocate_from_pools(conn, network_id, mac).await
}

/// Allocate IP for unknown device (no UUID mapping) within a specific network
pub async fn allocate_for_mac_in_network(
    conn: &Connection,
    mac: &str,
    network_id: i64,
) -> Result<Ipv4Addr> {
    // 1. Check static reservation in this network
    if let Some(reservation) = store::get_static_reservation(conn, network_id, mac).await? {
        log::debug!(
            "MAC {} has static reservation {} in network {}",
            mac,
            reservation.ip_address,
            network_id
        );
        return Ok(reservation.ip_address.parse()?);
    }

    // 2. Check existing lease in this network
    if let Some(lease) = store::get_lease_by_mac(conn, mac).await?
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
    allocate_from_pools(conn, network_id, mac).await
}

/// Allocate from pools within a network (try each pool until success)
async fn allocate_from_pools(conn: &Connection, network_id: i64, mac: &str) -> Result<Ipv4Addr> {
    let pools = store::list_pools_for_network(conn, network_id).await?;

    if pools.is_empty() {
        return Err(anyhow::anyhow!(
            "No pools configured for network {}",
            network_id
        ));
    }

    // Get all active IPs in this network
    let active_ips: HashSet<Ipv4Addr> = store::get_leases_by_network(conn, network_id)
        .await?
        .into_iter()
        .filter(|l| !l.is_expired())
        .filter_map(|l| l.ip_address.parse().ok())
        .collect();

    // Get all statically reserved IPs in this network
    let reserved_ips: HashSet<Ipv4Addr> = store::list_static_reservations(conn, network_id)
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
    use crate::database::{self, DatabaseConnectionFactory};
    use crate::dhcp::store::LeaseState;
    use crate::test_connection_factory;
    use std::sync::Arc;

    async fn create_test_db(
        factory: DatabaseConnectionFactory,
    ) -> (Arc<crate::database::Connection>, i64) {
        let db = Arc::new(database::run_migrations(&factory).await.unwrap());

        // Create test network
        let network = store::create_network(
            &db,
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
        store::create_pool(&db, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        (db, network.id)
    }

    #[tokio::test]
    async fn test_allocate_for_mac_in_network() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // Allocate IP in test network
        let ip = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), "10.0.0.100"); // First IP in test range
    }

    #[tokio::test]
    async fn test_allocate_reuses_existing_lease() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // First allocation
        let ip1 = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();

        // Create lease
        store::create_or_update_lease_with_network(
            &db,
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
        let ip2 = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip1, ip2);
    }

    #[tokio::test]
    async fn test_allocate_different_ips_for_different_macs() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac1 = "aa:bb:cc:dd:ee:ff";
        let mac2 = "11:22:33:44:55:66";

        let ip1 = allocate_for_mac_in_network(&db, mac1, network_id)
            .await
            .unwrap();
        store::create_or_update_lease_with_network(
            &db,
            mac1,
            &ip1,
            None,
            LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        let ip2 = allocate_for_mac_in_network(&db, mac2, network_id)
            .await
            .unwrap();

        assert_ne!(ip1, ip2);
        assert_eq!(ip1.to_string(), "10.0.0.100");
        assert_eq!(ip2.to_string(), "10.0.0.101");
    }

    #[tokio::test]
    async fn test_static_reservation_takes_priority() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac = "aa:bb:cc:dd:ee:ff";
        let static_ip = "10.0.0.50";

        // Create static reservation
        store::create_static_reservation(&db, network_id, mac, static_ip, None)
            .await
            .unwrap();

        // Allocation should return the static IP
        let ip = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), static_ip);
    }

    #[tokio::test]
    async fn test_static_reservation_overrides_existing_lease() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac = "aa:bb:cc:dd:ee:ff";

        // First, allocate IP from pool
        let ip1 = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip1.to_string(), "10.0.0.100"); // First IP in pool

        // Create an active lease for this IP
        store::create_or_update_lease_with_network(
            &db,
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
        store::create_static_reservation(&db, network_id, mac, static_ip, None)
            .await
            .unwrap();

        // Next allocation should return the static IP, not the existing lease IP
        let ip2 = allocate_for_mac_in_network(&db, mac, network_id)
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
