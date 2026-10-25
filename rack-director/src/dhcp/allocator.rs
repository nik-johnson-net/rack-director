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
    // 0. Check device NIC assignment: if the operator configured a static IP for this
    //    MAC/network combination, hand it out immediately ahead of any other check.
    //    The device UUID is known here, so this is scoped to a single device row (indexed
    //    `uuid` lookup) instead of scanning every device's `network_interfaces` JSON.
    if let Some(ip) =
        crate::director::store::get_configured_ip_for_device(conn, uuid, mac, network_id).await?
    {
        log::debug!(
            "Device {} NIC {} has configured IP {} in network {} — using it",
            uuid,
            mac,
            ip,
            network_id
        );
        return Ok(ip);
    }

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
    // 0. Check device NIC assignment ahead of everything else.
    if let Some(ip) =
        crate::director::store::get_configured_ip_for_mac(conn, mac, network_id).await?
    {
        log::debug!(
            "NIC {} has configured IP {} in network {} — using it",
            mac,
            ip,
            network_id
        );
        return Ok(ip);
    }

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

    // Get all operator-assigned NIC static IPs in this network (may fall inside a pool range)
    let assigned_ips: HashSet<Ipv4Addr> =
        crate::director::store::list_assigned_nic_ips(conn, network_id)
            .await?
            .into_iter()
            .filter_map(|ip| ip.parse().ok())
            .collect();

    // Try each pool until allocation succeeds
    for pool in pools {
        let range = parse_ip_range(&pool.range_start, &pool.range_end)?;

        for ip in range {
            if !active_ips.contains(&ip)
                && !reserved_ips.contains(&ip)
                && !assigned_ips.contains(&ip)
            {
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
            Some("10.0.0.1"),
            &["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            86400,
            None,
            false,
            None,
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

    /// A device NIC configured with an assigned_ip in a network receives that IP from
    /// the allocator ahead of pool allocation.
    #[tokio::test]
    async fn test_configured_nic_ip_takes_priority_over_pool() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let mac = "aa:bb:cc:dd:ee:10";

        // Register a device and set a NIC assignment
        let uuid = Uuid::parse_str("a1000000-0000-0000-0000-000000000010").unwrap();
        db.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();

        let nic_json = format!(
            r#"[{{"interface_name":"eth0","mac_address":"{mac}","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":{network_id},"assigned_ip":"10.0.0.42"}}]"#
        );
        db.execute(
            "UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.network_interfaces', json(?1)) WHERE uuid = ?2",
            (nic_json, uuid),
        )
        .await
        .unwrap();

        // Allocator must return the configured IP, not the first pool address (10.0.0.100)
        let ip = allocate_for_mac_in_network(&db, mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), "10.0.0.42");

        // Also verify via allocate_for_device_in_network
        let ip2 = allocate_for_device_in_network(&db, mac, &uuid, network_id)
            .await
            .unwrap();
        assert_eq!(ip2.to_string(), "10.0.0.42");
    }

    /// A pool allocation must not hand out an IP that is already claimed by another
    /// device's operator-assigned NIC static IP, even if that device has never DHCP'd
    /// (i.e. has no active lease).
    #[tokio::test]
    async fn test_pool_allocation_skips_assigned_nic_ip() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let device_a_mac = "aa:bb:cc:dd:ee:20";

        // Register device A and assign it the first pool address, but create no lease for it.
        let uuid = Uuid::parse_str("a1000000-0000-0000-0000-000000000020").unwrap();
        db.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();

        let nic_json = format!(
            r#"[{{"interface_name":"eth0","mac_address":"{device_a_mac}","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":{network_id},"assigned_ip":"10.0.0.100"}}]"#
        );
        db.execute(
            "UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.network_interfaces', json(?1)) WHERE uuid = ?2",
            (nic_json, uuid),
        )
        .await
        .unwrap();

        // Device B (a different MAC, no assignment) requests an allocation. The pool scan
        // must skip 10.0.0.100 (claimed by device A's assignment) and return the next
        // available address instead.
        let device_b_mac = "11:22:33:44:55:66";
        let ip = allocate_for_mac_in_network(&db, device_b_mac, network_id)
            .await
            .unwrap();
        assert_eq!(ip.to_string(), "10.0.0.101");
    }

    /// `list_assigned_nic_ips` returns only the assigned IPs for the given network,
    /// ignoring NICs with no assignment or an assignment to a different network.
    #[tokio::test]
    async fn test_list_assigned_nic_ips_returns_assigned_ips_for_network() {
        let (db, network_id) = create_test_db(test_connection_factory!()).await;
        let other_network_id = network_id + 1;

        let uuid = Uuid::parse_str("a1000000-0000-0000-0000-000000000030").unwrap();
        db.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();

        // One NIC assigned in `network_id`, one assigned to a different network, one
        // with no assignment at all.
        let nic_json = format!(
            r#"[
                {{"interface_name":"eth0","mac_address":"aa:bb:cc:dd:ee:30","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":{network_id},"assigned_ip":"10.0.0.150"}},
                {{"interface_name":"eth1","mac_address":"aa:bb:cc:dd:ee:31","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":{other_network_id},"assigned_ip":"10.0.1.5"}},
                {{"interface_name":"eth2","mac_address":"aa:bb:cc:dd:ee:32","ip_address":null,"network_id":null,"speed_mbps":null,"disabled":false,"warning_label":null,"assigned_network_id":null,"assigned_ip":null}}
            ]"#
        );
        db.execute(
            "UPDATE devices SET attributes = json_set(COALESCE(attributes, '{}'), '$.network_interfaces', json(?1)) WHERE uuid = ?2",
            (nic_json, uuid),
        )
        .await
        .unwrap();

        let assigned = crate::director::store::list_assigned_nic_ips(&db, network_id)
            .await
            .unwrap();
        assert_eq!(assigned, vec!["10.0.0.150".to_string()]);

        // No assignments exist for a network with no matching NICs.
        let none_assigned = crate::director::store::list_assigned_nic_ips(&db, other_network_id)
            .await
            .unwrap();
        assert_eq!(none_assigned, vec!["10.0.1.5".to_string()]);
    }
}
