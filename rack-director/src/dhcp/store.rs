use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::database::FromRow;

#[derive(Debug, Clone)]
pub struct DhcpStore {
    db: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub id: i64,
    pub mac_address: String,
    pub ip_address: String,
    pub device_uuid: Option<Uuid>,
    pub lease_start: DateTime<Utc>,
    pub lease_end: DateTime<Utc>,
    pub state: LeaseState,
    pub hostname: Option<String>,
    pub network_id: Option<i64>,
}

impl FromRow for Lease {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let lease_start_str: String = row.get("lease_start")?;
        let lease_end_str: String = row.get("lease_end")?;
        let state_str: String = row.get("state")?;

        Ok(Lease {
            id: row.get("id")?,
            mac_address: row.get("mac_address")?,
            ip_address: row.get("ip_address")?,
            device_uuid: row.get("device_uuid")?,
            lease_start: lease_start_str.parse().unwrap(),
            lease_end: lease_end_str.parse().unwrap(),
            state: state_str.parse().unwrap(),
            hostname: row.get("hostname")?,
            network_id: row.get("network_id")?,
        })
    }
}

impl Lease {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.lease_end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseState {
    Offered,
    Active,
    Expired,
    Released,
}

impl std::fmt::Display for LeaseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseState::Offered => write!(f, "offered"),
            LeaseState::Active => write!(f, "active"),
            LeaseState::Expired => write!(f, "expired"),
            LeaseState::Released => write!(f, "released"),
        }
    }
}

impl std::str::FromStr for LeaseState {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "offered" => Ok(LeaseState::Offered),
            "active" => Ok(LeaseState::Active),
            "expired" => Ok(LeaseState::Expired),
            "released" => Ok(LeaseState::Released),
            _ => Err(anyhow::anyhow!("Invalid lease state: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpNetwork {
    pub id: i64,
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub lease_duration: u32,
    pub relay_agent_address: Option<String>,
    pub enable_autodiscovery: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FromRow for DhcpNetwork {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let dns_servers_json: String = row.get("dns_servers")?;
        let dns_servers: Vec<String> =
            serde_json::from_str(&dns_servers_json).unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

        let created_at_str: String = row.get("created_at")?;
        let updated_at_str: String = row.get("updated_at")?;

        Ok(DhcpNetwork {
            id: row.get("id")?,
            name: row.get("name")?,
            subnet: row.get("subnet")?,
            gateway: row.get("gateway")?,
            dns_servers,
            lease_duration: row.get("lease_duration")?,
            relay_agent_address: row.get("relay_agent_address")?,
            enable_autodiscovery: row.get("enable_autodiscovery")?,
            created_at: parse_datetime(&created_at_str).unwrap(),
            updated_at: parse_datetime(&updated_at_str).unwrap(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpPool {
    pub id: i64,
    pub network_id: i64,
    pub name: String,
    pub range_start: String,
    pub range_end: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FromRow for DhcpPool {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let created_at_str: String = row.get("created_at")?;
        let updated_at_str: String = row.get("updated_at")?;

        Ok(DhcpPool {
            id: row.get("id")?,
            network_id: row.get("network_id")?,
            name: row.get("name")?,
            range_start: row.get("range_start")?,
            range_end: row.get("range_end")?,
            created_at: parse_datetime(&created_at_str).unwrap(),
            updated_at: parse_datetime(&updated_at_str).unwrap(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticReservation {
    pub id: i64,
    pub network_id: i64,
    pub mac_address: String,
    pub ip_address: String,
    pub hostname: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FromRow for StaticReservation {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let created_at_str: String = row.get("created_at")?;
        let updated_at_str: String = row.get("updated_at")?;

        Ok(StaticReservation {
            id: row.get("id")?,
            network_id: row.get("network_id")?,
            mac_address: row.get("mac_address")?,
            ip_address: row.get("ip_address")?,
            hostname: row.get("hostname")?,
            created_at: parse_datetime(&created_at_str).unwrap(),
            updated_at: parse_datetime(&updated_at_str).unwrap(),
        })
    }
}

impl DhcpStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Create or update a DHCP lease with network context
    pub async fn create_or_update_lease_with_network(
        &self,
        mac: &str,
        ip: &Ipv4Addr,
        device_uuid: Option<&Uuid>,
        state: LeaseState,
        lease_duration: u32,
        network_id: i64,
    ) -> Result<()> {
        let ip_str = ip.to_string();
        let now = Utc::now();
        let lease_end = now + Duration::seconds(lease_duration as i64);

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_leases
                (mac_address, ip_address, device_uuid, lease_start, lease_end, state, network_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(mac_address) DO UPDATE SET
                ip_address = ?2,
                device_uuid = ?3,
                lease_start = ?4,
                lease_end = ?5,
                state = ?6,
                network_id = ?7,
                updated_at = ?8",
            params![
                mac,
                ip_str,
                device_uuid,
                now.to_rfc3339(),
                lease_end.to_rfc3339(),
                state.to_string(),
                network_id,
                now.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Get lease by MAC address
    pub async fn get_lease_by_mac(&self, mac: &str) -> Result<Option<Lease>> {
        let db = self.db.lock().await;

        let lease = crate::database::query_optional::<Lease>(
            &db,
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE mac_address = ?",
            &[&mac],
        )?;

        Ok(lease)
    }

    /// Get lease by ID
    pub async fn get_lease_by_id(&self, id: i64) -> Result<Option<Lease>> {
        let db = self.db.lock().await;

        let lease = crate::database::query_optional::<Lease>(
            &db,
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE id = ?",
            &[&id],
        )?;

        Ok(lease)
    }

    /// Activate a lease (transition from Offered to Active)
    pub async fn activate_lease(&self, mac: &str) -> Result<()> {
        let db = self.db.lock().await;

        db.execute(
            "UPDATE dhcp_leases SET state = ?1, updated_at = ?2 WHERE mac_address = ?3",
            params![LeaseState::Active.to_string(), Utc::now().to_rfc3339(), mac],
        )?;

        Ok(())
    }

    /// Release a lease (mark as Released)
    pub async fn release_lease(&self, mac: &str) -> Result<()> {
        let db = self.db.lock().await;

        db.execute(
            "UPDATE dhcp_leases SET state = ?1, updated_at = ?2 WHERE mac_address = ?3",
            params![
                LeaseState::Released.to_string(),
                Utc::now().to_rfc3339(),
                mac
            ],
        )?;

        Ok(())
    }

    /// Get all leases (for API/management)
    pub async fn get_all_leases(&self) -> Result<Vec<Lease>> {
        let db = self.db.lock().await;

        let leases = crate::database::query_map_all::<Lease>(
            &db,
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases ORDER BY updated_at DESC",
            &[],
        )?;

        Ok(leases)
    }

    /// Find lease by device UUID (synchronous for use in non-async contexts)
    pub fn find_lease_by_device_uuid(&self, device_uuid: &Uuid) -> Result<Option<Lease>> {
        // Get the db without using async
        let db = self
            .db
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Could not lock database"))?;

        let lease = crate::database::query_optional::<Lease>(
            &db,
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE device_uuid = ? AND state = 'active' ORDER BY lease_end DESC LIMIT 1",
            &[device_uuid],
        )?;

        Ok(lease)
    }

    // ========== Network CRUD Operations ==========

    /// Get a network by ID
    pub async fn get_network(&self, id: i64) -> Result<DhcpNetwork> {
        let db = self.db.lock().await;

        let network = crate::database::query_one::<DhcpNetwork>(
            &db,
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
             FROM dhcp_networks WHERE id = ?",
            &[&id],
        )?;

        Ok(network)
    }

    /// Get a network by relay agent address (or None for local L2)
    pub async fn get_network_by_relay(
        &self,
        relay: Option<Ipv4Addr>,
    ) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;
        let relay_str = relay.map(|r| r.to_string());

        let network = crate::database::query_optional::<DhcpNetwork>(
            &db,
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
             FROM dhcp_networks WHERE relay_agent_address IS ? OR (relay_agent_address IS NULL AND ? IS NULL)",
            &[&relay_str, &relay_str],
        )?;

        Ok(network)
    }

    /// Get a network by name
    pub async fn get_network_by_name(&self, name: &str) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;

        let network = crate::database::query_optional::<DhcpNetwork>(
            &db,
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
             FROM dhcp_networks WHERE name = ?",
            &[&name],
        )?;

        Ok(network)
    }

    /// Get a network by relay agent address string (checking both NULL and empty string for local L2)
    pub async fn get_network_by_relay_string(
        &self,
        relay_agent_address: Option<&str>,
    ) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;

        // Handle the three cases:
        // 1. None or Some("") - Local L2 network (NULL or empty string)
        // 2. Some(address) - Specific relay agent address
        let network = match relay_agent_address {
            None | Some("") => crate::database::query_optional::<DhcpNetwork>(
                &db,
                "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                 FROM dhcp_networks WHERE relay_agent_address IS NULL OR relay_agent_address = ''",
                &[],
            )?,
            Some(addr) => {
                let addr_string = addr.to_string();
                crate::database::query_optional::<DhcpNetwork>(
                    &db,
                    "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                     FROM dhcp_networks WHERE relay_agent_address = ?",
                    &[&addr_string],
                )?
            }
        };

        Ok(network)
    }

    /// List all networks
    pub async fn list_networks(&self) -> Result<Vec<DhcpNetwork>> {
        let db = self.db.lock().await;

        let networks = crate::database::query_map_all::<DhcpNetwork>(
            &db,
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
             FROM dhcp_networks ORDER BY name",
            &[],
        )?;

        Ok(networks)
    }

    /// Create a new network
    #[allow(clippy::too_many_arguments)]
    pub async fn create_network(
        &self,
        name: &str,
        subnet: &str,
        gateway: &str,
        dns_servers: &[String],
        lease_duration: u32,
        relay_agent_address: Option<&str>,
        enable_autodiscovery: bool,
    ) -> Result<DhcpNetwork> {
        let dns_servers_json = serde_json::to_string(dns_servers)?;
        let now = Utc::now().to_rfc3339();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_networks (name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![name, subnet, gateway, dns_servers_json, lease_duration, relay_agent_address, enable_autodiscovery, now, now],
        )?;

        let id = db.last_insert_rowid();
        drop(db);

        self.get_network(id).await
    }

    /// Update a network
    #[allow(clippy::too_many_arguments)]
    pub async fn update_network(
        &self,
        id: i64,
        name: Option<&str>,
        subnet: Option<&str>,
        gateway: Option<&str>,
        dns_servers: Option<&[String]>,
        lease_duration: Option<u32>,
        relay_agent_address: Option<Option<&str>>,
        enable_autodiscovery: Option<bool>,
    ) -> Result<DhcpNetwork> {
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().await;

        if let Some(name) = name {
            db.execute(
                "UPDATE dhcp_networks SET name = ?, updated_at = ? WHERE id = ?",
                params![name, now, id],
            )?;
        }
        if let Some(subnet) = subnet {
            db.execute(
                "UPDATE dhcp_networks SET subnet = ?, updated_at = ? WHERE id = ?",
                params![subnet, now, id],
            )?;
        }
        if let Some(gateway) = gateway {
            db.execute(
                "UPDATE dhcp_networks SET gateway = ?, updated_at = ? WHERE id = ?",
                params![gateway, now, id],
            )?;
        }
        if let Some(dns_servers) = dns_servers {
            let dns_servers_json = serde_json::to_string(dns_servers)?;
            db.execute(
                "UPDATE dhcp_networks SET dns_servers = ?, updated_at = ? WHERE id = ?",
                params![dns_servers_json, now, id],
            )?;
        }
        if let Some(lease_duration) = lease_duration {
            db.execute(
                "UPDATE dhcp_networks SET lease_duration = ?, updated_at = ? WHERE id = ?",
                params![lease_duration, now, id],
            )?;
        }
        if let Some(relay_agent_address) = relay_agent_address {
            db.execute(
                "UPDATE dhcp_networks SET relay_agent_address = ?, updated_at = ? WHERE id = ?",
                params![relay_agent_address, now, id],
            )?;
        }
        if let Some(enable_autodiscovery) = enable_autodiscovery {
            db.execute(
                "UPDATE dhcp_networks SET enable_autodiscovery = ?, updated_at = ? WHERE id = ?",
                params![enable_autodiscovery, now, id],
            )?;
        }

        drop(db);
        self.get_network(id).await
    }

    /// Delete a network
    pub async fn delete_network(&self, id: i64) -> Result<()> {
        let db = self.db.lock().await;
        db.execute("DELETE FROM dhcp_networks WHERE id = ?", params![id])?;
        Ok(())
    }

    // ========== Pool CRUD Operations ==========

    /// Get a pool by ID
    pub async fn get_pool(&self, id: i64) -> Result<DhcpPool> {
        let db = self.db.lock().await;

        let pool = crate::database::query_one::<DhcpPool>(
            &db,
            "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
             FROM dhcp_pools WHERE id = ?",
            &[&id],
        )?;

        Ok(pool)
    }

    /// List all pools for a network
    pub async fn list_pools_for_network(&self, network_id: i64) -> Result<Vec<DhcpPool>> {
        let db = self.db.lock().await;

        let pools = crate::database::query_map_all::<DhcpPool>(
            &db,
            "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
             FROM dhcp_pools WHERE network_id = ? ORDER BY name",
            &[&network_id],
        )?;

        Ok(pools)
    }

    /// Create a new pool
    pub async fn create_pool(
        &self,
        network_id: i64,
        name: &str,
        range_start: &str,
        range_end: &str,
    ) -> Result<DhcpPool> {
        let now = Utc::now().to_rfc3339();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_pools (network_id, name, range_start, range_end, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![network_id, name, range_start, range_end, now, now],
        )?;

        let id = db.last_insert_rowid();
        drop(db);

        self.get_pool(id).await
    }

    /// Update a pool
    pub async fn update_pool(
        &self,
        id: i64,
        name: Option<&str>,
        range_start: Option<&str>,
        range_end: Option<&str>,
    ) -> Result<DhcpPool> {
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().await;

        if let Some(name) = name {
            db.execute(
                "UPDATE dhcp_pools SET name = ?, updated_at = ? WHERE id = ?",
                params![name, now, id],
            )?;
        }
        if let Some(range_start) = range_start {
            db.execute(
                "UPDATE dhcp_pools SET range_start = ?, updated_at = ? WHERE id = ?",
                params![range_start, now, id],
            )?;
        }
        if let Some(range_end) = range_end {
            db.execute(
                "UPDATE dhcp_pools SET range_end = ?, updated_at = ? WHERE id = ?",
                params![range_end, now, id],
            )?;
        }

        drop(db);
        self.get_pool(id).await
    }

    /// Delete a pool
    pub async fn delete_pool(&self, id: i64) -> Result<()> {
        let db = self.db.lock().await;
        db.execute("DELETE FROM dhcp_pools WHERE id = ?", params![id])?;
        Ok(())
    }

    // ========== Static Reservation CRUD Operations ==========

    /// Get a static reservation for a MAC address in a network
    pub async fn get_static_reservation(
        &self,
        network_id: i64,
        mac: &str,
    ) -> Result<Option<StaticReservation>> {
        let db = self.db.lock().await;

        let reservation = crate::database::query_optional::<StaticReservation>(
            &db,
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE network_id = ? AND mac_address = ?",
            &[&network_id, &mac],
        )?;

        Ok(reservation)
    }

    /// List all static reservations for a network
    pub async fn list_static_reservations(
        &self,
        network_id: i64,
    ) -> Result<Vec<StaticReservation>> {
        let db = self.db.lock().await;

        let reservations = crate::database::query_map_all::<StaticReservation>(
            &db,
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE network_id = ? ORDER BY ip_address",
            &[&network_id],
        )?;

        Ok(reservations)
    }

    /// Create a static reservation
    pub async fn create_static_reservation(
        &self,
        network_id: i64,
        mac: &str,
        ip: &str,
        hostname: Option<&str>,
    ) -> Result<StaticReservation> {
        let now = Utc::now().to_rfc3339();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_static_reservations (network_id, mac_address, ip_address, hostname, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![network_id, mac, ip, hostname, now, now],
        )?;

        let id = db.last_insert_rowid();

        // Fetch and return the created reservation
        let reservation = crate::database::query_one::<StaticReservation>(
            &db,
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE id = ?",
            &[&id],
        )?;

        Ok(reservation)
    }

    /// Delete a static reservation
    pub async fn delete_static_reservation(&self, id: i64) -> Result<()> {
        let db = self.db.lock().await;
        db.execute(
            "DELETE FROM dhcp_static_reservations WHERE id = ?",
            params![id],
        )?;
        Ok(())
    }

    /// Get leases by network
    pub async fn get_leases_by_network(&self, network_id: i64) -> Result<Vec<Lease>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE network_id = ? ORDER BY updated_at DESC",
        )?;

        let leases = stmt
            .query_map(params![network_id], |row| {
                Ok(Lease {
                    id: row.get(0)?,
                    mac_address: row.get(1)?,
                    ip_address: row.get(2)?,
                    device_uuid: row.get(3)?,
                    lease_start: row.get::<_, String>(4)?.parse().unwrap(),
                    lease_end: row.get::<_, String>(5)?.parse().unwrap(),
                    state: row.get::<_, String>(6)?.parse().unwrap(),
                    hostname: row.get(7)?,
                    network_id: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(leases)
    }

    /// Delete all expired DHCP leases from the database.
    /// Deletes leases in any state where lease_end < now.
    /// Returns the number of deleted leases.
    pub async fn delete_expired_leases(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().await;
        let deleted = db.execute("DELETE FROM dhcp_leases WHERE lease_end < ?1", params![now])?;
        Ok(deleted as u64)
    }
}

pub fn format_mac(mac: &[u8]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

/// Parse datetime from SQLite's CURRENT_TIMESTAMP format or RFC3339
fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try SQLite's CURRENT_TIMESTAMP format: "YYYY-MM-DD HH:MM:SS"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
    }

    Err(anyhow::anyhow!("Failed to parse datetime: {}", s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> (DhcpStore, i64, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(db_path).unwrap();
        let store = DhcpStore::new(Arc::new(Mutex::new(conn)));

        // Create test network
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

        (store, network.id, temp_dir)
    }

    #[tokio::test]
    async fn test_get_network() {
        let (store, network_id, _temp_dir) = create_test_store().await;
        let network = store.get_network(network_id).await.unwrap();
        assert_eq!(network.name, "Test Network");
        assert_eq!(network.subnet, "10.0.0.0/24");
        assert_eq!(network.gateway, "10.0.0.1");
    }

    #[tokio::test]
    async fn test_get_network_by_name() {
        let (store, _network_id, _temp_dir) = create_test_store().await;

        // Test existing network
        let network = store.get_network_by_name("Test Network").await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Test Network");

        // Test non-existent network
        let network = store.get_network_by_name("NonExistent").await.unwrap();
        assert!(network.is_none());
    }

    #[tokio::test]
    async fn test_get_network_by_relay_string() {
        let (store, _network_id, _temp_dir) = create_test_store().await;

        // Create a network with a relay agent
        store
            .create_network(
                "Relay Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Test finding by specific relay agent address
        let network = store
            .get_network_by_relay_string(Some("10.0.0.2"))
            .await
            .unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Relay Network");
        assert_eq!(network.relay_agent_address, Some("10.0.0.2".to_string()));

        // Test finding local L2 network (None)
        let network = store.get_network_by_relay_string(None).await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Test Network");
        assert!(network.relay_agent_address.is_none());

        // Test finding local L2 network (empty string)
        let network = store.get_network_by_relay_string(Some("")).await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Test Network");

        // Test non-existent relay agent
        let network = store
            .get_network_by_relay_string(Some("10.0.0.99"))
            .await
            .unwrap();
        assert!(network.is_none());
    }

    #[tokio::test]
    async fn test_format_mac() {
        let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(format_mac(&mac), "aa:bb:cc:dd:ee:ff");
    }

    #[tokio::test]
    async fn test_delete_expired_leases_removes_expired() {
        let (store, network_id, _temp_dir) = create_test_store().await;

        // Insert a lease that expired 1 hour ago
        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let db = store.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:01", "10.0.0.101", expired_time, expired_time, "active", network_id],
        ).unwrap();
        drop(db);

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 1);

        // Verify the lease is gone
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_expired_leases_preserves_active() {
        let (store, network_id, _temp_dir) = create_test_store().await;

        // Insert a lease that expires in 1 hour
        let future_time = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let now = Utc::now().to_rfc3339();
        let db = store.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:02", "10.0.0.102", now, future_time, "active", network_id],
        ).unwrap();
        drop(db);

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 0);

        // Verify the lease still exists
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].mac_address, "aa:bb:cc:dd:ee:02");
    }

    #[tokio::test]
    async fn test_delete_expired_leases_mixed() {
        let (store, network_id, _temp_dir) = create_test_store().await;

        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let future_time = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let now = Utc::now().to_rfc3339();

        let db = store.db.lock().await;
        // Insert 2 expired leases
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:03", "10.0.0.103", expired_time, expired_time, "active", network_id],
        ).unwrap();
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:04", "10.0.0.104", expired_time, expired_time, "offered", network_id],
        ).unwrap();
        // Insert 1 active lease
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:05", "10.0.0.105", now, future_time, "active", network_id],
        ).unwrap();
        drop(db);

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 2);

        // Verify only the active lease remains
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].mac_address, "aa:bb:cc:dd:ee:05");
    }

    #[tokio::test]
    async fn test_delete_expired_leases_no_leases() {
        let (store, _network_id, _temp_dir) = create_test_store().await;

        // Delete expired leases when there are none
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_delete_expired_leases_released_and_expired() {
        let (store, network_id, _temp_dir) = create_test_store().await;

        // Insert a released lease that is also expired
        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let db = store.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?, ?, ?, ?, ?, ?)",
            params!["aa:bb:cc:dd:ee:06", "10.0.0.106", expired_time, expired_time, "released", network_id],
        ).unwrap();
        drop(db);

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 1);

        // Verify the lease is gone
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 0);
    }
}
