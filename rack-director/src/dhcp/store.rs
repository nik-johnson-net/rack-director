use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;
use uuid::Uuid;

use crate::database::{Connection, FromRow};

#[derive(Clone)]
pub struct DhcpStore {
    db: Arc<Connection>,
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
    pub fn new(db: Arc<Connection>) -> Self {
        Self { db }
    }

    /// Create or update a DHCP lease with network context.
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
        let state_str = state.to_string();
        let now_str = now.to_rfc3339();
        let lease_end_str = lease_end.to_rfc3339();
        let device_uuid_copy = device_uuid.copied();
        let mac = mac.to_string();

        self.db
            .execute(
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
                (mac, ip_str, device_uuid_copy, now_str.clone(), lease_end_str, state_str, network_id, now_str),
            )
            .await?;

        Ok(())
    }

    /// Get lease by MAC address.
    pub async fn get_lease_by_mac(&self, mac: &str) -> Result<Option<Lease>> {
        let lease = self
            .db
            .query_row(
                "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
                 FROM dhcp_leases WHERE mac_address = ?1",
                (mac.to_string(),),
                Lease::from_row,
            )
            .await
            .optional()?;

        Ok(lease)
    }

    /// Get lease by ID.
    pub async fn get_lease_by_id(&self, id: i64) -> Result<Option<Lease>> {
        let lease = self
            .db
            .query_row(
                "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
                 FROM dhcp_leases WHERE id = ?1",
                (id,),
                Lease::from_row,
            )
            .await
            .optional()?;

        Ok(lease)
    }

    /// Activate a lease (transition from Offered to Active).
    pub async fn activate_lease(&self, mac: &str) -> Result<()> {
        self.db
            .execute(
                "UPDATE dhcp_leases SET state = ?1, updated_at = ?2 WHERE mac_address = ?3",
                (
                    LeaseState::Active.to_string(),
                    Utc::now().to_rfc3339(),
                    mac.to_string(),
                ),
            )
            .await?;

        Ok(())
    }

    /// Release a lease (mark as Released).
    pub async fn release_lease(&self, mac: &str) -> Result<()> {
        self.db
            .execute(
                "UPDATE dhcp_leases SET state = ?1, updated_at = ?2 WHERE mac_address = ?3",
                (
                    LeaseState::Released.to_string(),
                    Utc::now().to_rfc3339(),
                    mac.to_string(),
                ),
            )
            .await?;

        Ok(())
    }

    /// Get all leases (for API/management).
    pub async fn get_all_leases(&self) -> Result<Vec<Lease>> {
        let leases = self
            .db
            .query(
                "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
                 FROM dhcp_leases ORDER BY updated_at DESC",
                (),
                Lease::from_row,
            )
            .await?;

        Ok(leases)
    }

    /// Find lease by device UUID (async version).
    pub async fn find_lease_by_device_uuid(&self, device_uuid: &Uuid) -> Result<Option<Lease>> {
        let lease = self
            .db
            .query_row(
                "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
                 FROM dhcp_leases WHERE device_uuid = ?1 AND state = 'active' ORDER BY lease_end DESC LIMIT 1",
                (*device_uuid,),
                Lease::from_row,
            )
            .await
            .optional()?;

        Ok(lease)
    }

    // ========== Network CRUD Operations ==========

    /// Get a network by ID.
    pub async fn get_network(&self, id: i64) -> Result<DhcpNetwork> {
        let network = self
            .db
            .query_one(
                "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                 FROM dhcp_networks WHERE id = ?1",
                (id,),
                DhcpNetwork::from_row,
            )
            .await?;

        Ok(network)
    }

    /// Get a network by relay agent address (or None for local L2).
    pub async fn get_network_by_relay(
        &self,
        relay: Option<Ipv4Addr>,
    ) -> Result<Option<DhcpNetwork>> {
        let relay_str = relay.map(|r| r.to_string());

        let network = self
            .db
            .query_row(
                "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                 FROM dhcp_networks WHERE relay_agent_address IS ?1 OR (relay_agent_address IS NULL AND ?1 IS NULL)",
                (relay_str,),
                DhcpNetwork::from_row,
            )
            .await
            .optional()?;

        Ok(network)
    }

    /// Get a network by name.
    pub async fn get_network_by_name(&self, name: &str) -> Result<Option<DhcpNetwork>> {
        let network = self
            .db
            .query_row(
                "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                 FROM dhcp_networks WHERE name = ?1",
                (name.to_string(),),
                DhcpNetwork::from_row,
            )
            .await
            .optional()?;

        Ok(network)
    }

    /// Get a network by relay agent address string, checking both NULL and empty string for local L2.
    pub async fn get_network_by_relay_string(
        &self,
        relay_agent_address: Option<&str>,
    ) -> Result<Option<DhcpNetwork>> {
        // Handle the three cases:
        // 1. None or Some("") - Local L2 network (NULL or empty string)
        // 2. Some(address) - Specific relay agent address
        let network = match relay_agent_address {
            None | Some("") => self
                .db
                .query_row(
                    "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                     FROM dhcp_networks WHERE relay_agent_address IS NULL OR relay_agent_address = ''",
                    (),
                    DhcpNetwork::from_row,
                )
                .await
                .optional()?,
            Some(addr) => self
                .db
                .query_row(
                    "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                     FROM dhcp_networks WHERE relay_agent_address = ?1",
                    (addr.to_string(),),
                    DhcpNetwork::from_row,
                )
                .await
                .optional()?,
        };

        Ok(network)
    }

    /// List all networks.
    pub async fn list_networks(&self) -> Result<Vec<DhcpNetwork>> {
        let networks = self
            .db
            .query(
                "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at
                 FROM dhcp_networks ORDER BY name",
                (),
                DhcpNetwork::from_row,
            )
            .await?;

        Ok(networks)
    }

    /// Create a new network.
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
        let relay = relay_agent_address.map(|s| s.to_string());

        self.db
            .execute(
                "INSERT INTO dhcp_networks (name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, enable_autodiscovery, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (name.to_string(), subnet.to_string(), gateway.to_string(), dns_servers_json, lease_duration, relay, enable_autodiscovery, now.clone(), now),
            )
            .await?;

        let id = self.db.last_insert_rowid().await;
        self.get_network(id).await
    }

    /// Update a network.
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

        if let Some(name) = name {
            self.db
                .execute(
                    "UPDATE dhcp_networks SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    (name.to_string(), now.clone(), id),
                )
                .await?;
        }
        if let Some(subnet) = subnet {
            self.db
                .execute(
                    "UPDATE dhcp_networks SET subnet = ?1, updated_at = ?2 WHERE id = ?3",
                    (subnet.to_string(), now.clone(), id),
                )
                .await?;
        }
        if let Some(gateway) = gateway {
            self.db
                .execute(
                    "UPDATE dhcp_networks SET gateway = ?1, updated_at = ?2 WHERE id = ?3",
                    (gateway.to_string(), now.clone(), id),
                )
                .await?;
        }
        if let Some(dns_servers) = dns_servers {
            let dns_servers_json = serde_json::to_string(dns_servers)?;
            self.db
                .execute(
                    "UPDATE dhcp_networks SET dns_servers = ?1, updated_at = ?2 WHERE id = ?3",
                    (dns_servers_json, now.clone(), id),
                )
                .await?;
        }
        if let Some(lease_duration) = lease_duration {
            self.db
                .execute(
                    "UPDATE dhcp_networks SET lease_duration = ?1, updated_at = ?2 WHERE id = ?3",
                    (lease_duration, now.clone(), id),
                )
                .await?;
        }
        if let Some(relay_agent_address) = relay_agent_address {
            let relay = relay_agent_address.map(|s| s.to_string());
            self.db
                .execute(
                    "UPDATE dhcp_networks SET relay_agent_address = ?1, updated_at = ?2 WHERE id = ?3",
                    (relay, now.clone(), id),
                )
                .await?;
        }
        if let Some(enable_autodiscovery) = enable_autodiscovery {
            self.db
                .execute(
                    "UPDATE dhcp_networks SET enable_autodiscovery = ?1, updated_at = ?2 WHERE id = ?3",
                    (enable_autodiscovery, now, id),
                )
                .await?;
        }

        self.get_network(id).await
    }

    /// Delete a network.
    pub async fn delete_network(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM dhcp_networks WHERE id = ?1", (id,))
            .await?;
        Ok(())
    }

    // ========== Pool CRUD Operations ==========

    /// Get a pool by ID.
    pub async fn get_pool(&self, id: i64) -> Result<DhcpPool> {
        let pool = self
            .db
            .query_one(
                "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
                 FROM dhcp_pools WHERE id = ?1",
                (id,),
                DhcpPool::from_row,
            )
            .await?;

        Ok(pool)
    }

    /// List all pools for a network.
    pub async fn list_pools_for_network(&self, network_id: i64) -> Result<Vec<DhcpPool>> {
        let pools = self
            .db
            .query(
                "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
                 FROM dhcp_pools WHERE network_id = ?1 ORDER BY name",
                (network_id,),
                DhcpPool::from_row,
            )
            .await?;

        Ok(pools)
    }

    /// Create a new pool.
    pub async fn create_pool(
        &self,
        network_id: i64,
        name: &str,
        range_start: &str,
        range_end: &str,
    ) -> Result<DhcpPool> {
        let now = Utc::now().to_rfc3339();

        self.db
            .execute(
                "INSERT INTO dhcp_pools (network_id, name, range_start, range_end, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (network_id, name.to_string(), range_start.to_string(), range_end.to_string(), now.clone(), now),
            )
            .await?;

        let id = self.db.last_insert_rowid().await;
        self.get_pool(id).await
    }

    /// Update a pool.
    pub async fn update_pool(
        &self,
        id: i64,
        name: Option<&str>,
        range_start: Option<&str>,
        range_end: Option<&str>,
    ) -> Result<DhcpPool> {
        let now = Utc::now().to_rfc3339();

        if let Some(name) = name {
            self.db
                .execute(
                    "UPDATE dhcp_pools SET name = ?1, updated_at = ?2 WHERE id = ?3",
                    (name.to_string(), now.clone(), id),
                )
                .await?;
        }
        if let Some(range_start) = range_start {
            self.db
                .execute(
                    "UPDATE dhcp_pools SET range_start = ?1, updated_at = ?2 WHERE id = ?3",
                    (range_start.to_string(), now.clone(), id),
                )
                .await?;
        }
        if let Some(range_end) = range_end {
            self.db
                .execute(
                    "UPDATE dhcp_pools SET range_end = ?1, updated_at = ?2 WHERE id = ?3",
                    (range_end.to_string(), now, id),
                )
                .await?;
        }

        self.get_pool(id).await
    }

    /// Delete a pool.
    pub async fn delete_pool(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM dhcp_pools WHERE id = ?1", (id,))
            .await?;
        Ok(())
    }

    // ========== Static Reservation CRUD Operations ==========

    /// Get a static reservation for a MAC address in a network.
    pub async fn get_static_reservation(
        &self,
        network_id: i64,
        mac: &str,
    ) -> Result<Option<StaticReservation>> {
        let reservation = self
            .db
            .query_row(
                "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
                 FROM dhcp_static_reservations WHERE network_id = ?1 AND mac_address = ?2",
                (network_id, mac.to_string()),
                StaticReservation::from_row,
            )
            .await
            .optional()?;

        Ok(reservation)
    }

    /// List all static reservations for a network.
    pub async fn list_static_reservations(
        &self,
        network_id: i64,
    ) -> Result<Vec<StaticReservation>> {
        let reservations = self
            .db
            .query(
                "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
                 FROM dhcp_static_reservations WHERE network_id = ?1 ORDER BY ip_address",
                (network_id,),
                StaticReservation::from_row,
            )
            .await?;

        Ok(reservations)
    }

    /// Create a static reservation.
    pub async fn create_static_reservation(
        &self,
        network_id: i64,
        mac: &str,
        ip: &str,
        hostname: Option<&str>,
    ) -> Result<StaticReservation> {
        let now = Utc::now().to_rfc3339();
        let hostname_owned = hostname.map(|s| s.to_string());

        self.db
            .execute(
                "INSERT INTO dhcp_static_reservations (network_id, mac_address, ip_address, hostname, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (network_id, mac.to_string(), ip.to_string(), hostname_owned, now.clone(), now),
            )
            .await?;

        let id = self.db.last_insert_rowid().await;

        // Fetch and return the created reservation
        let reservation = self
            .db
            .query_one(
                "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
                 FROM dhcp_static_reservations WHERE id = ?1",
                (id,),
                StaticReservation::from_row,
            )
            .await?;

        Ok(reservation)
    }

    /// Delete a static reservation.
    pub async fn delete_static_reservation(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM dhcp_static_reservations WHERE id = ?1", (id,))
            .await?;
        Ok(())
    }

    /// Create or update a static reservation (upsert).
    ///
    /// Creates a new static reservation or updates an existing one if the MAC address
    /// already has a reservation in the specified network. This makes the operation
    /// idempotent and safe to call multiple times during device registration.
    ///
    /// # Errors
    /// Returns an error if the IP address is already reserved for a different MAC on this network.
    pub async fn create_or_update_static_reservation(
        &self,
        network_id: i64,
        mac: &str,
        ip: &str,
        hostname: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let hostname_owned = hostname.map(|s| s.to_string());

        self.db
            .execute(
                "INSERT INTO dhcp_static_reservations (network_id, mac_address, ip_address, hostname, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(network_id, mac_address) DO UPDATE SET
                    ip_address = ?3,
                    hostname = ?4,
                    updated_at = ?6",
                (network_id, mac.to_string(), ip.to_string(), hostname_owned, now.clone(), now),
            )
            .await?;

        Ok(())
    }

    /// Delete all static reservations for a MAC address across all networks.
    ///
    /// This is a bulk deletion operation used when a device is deleted. It removes all
    /// reservations for the given MAC address regardless of which network they're on.
    ///
    /// Returns the number of reservations deleted (0 if the MAC has no reservations).
    pub async fn delete_static_reservations_by_mac(&self, mac: &str) -> Result<u64> {
        let deleted = self
            .db
            .execute(
                "DELETE FROM dhcp_static_reservations WHERE mac_address = ?1",
                (mac.to_string(),),
            )
            .await?;
        Ok(deleted as u64)
    }

    /// Get leases by network.
    pub async fn get_leases_by_network(&self, network_id: i64) -> Result<Vec<Lease>> {
        let leases = self
            .db
            .query(
                "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
                 FROM dhcp_leases WHERE network_id = ?1 ORDER BY updated_at DESC",
                (network_id,),
                Lease::from_row,
            )
            .await?;

        Ok(leases)
    }

    /// Delete all expired DHCP leases from the database.
    ///
    /// Deletes leases in any state where lease_end < now. Returns the number of deleted leases.
    pub async fn delete_expired_leases(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let deleted = self
            .db
            .execute("DELETE FROM dhcp_leases WHERE lease_end < ?1", (now,))
            .await?;
        Ok(deleted as u64)
    }
}

pub fn format_mac(mac: &[u8]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

/// Parse datetime from SQLite's CURRENT_TIMESTAMP format or RFC3339.
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
    use crate::{database::open, test_database_path};

    use super::*;

    async fn create_test_store(path: String) -> (DhcpStore, i64) {
        let db = Arc::new(open(path).await.unwrap());
        let store = DhcpStore::new(db);

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

        (store, network.id)
    }

    #[tokio::test]
    async fn test_get_network() {
        let (store, network_id) = create_test_store(test_database_path!()).await;
        let network = store.get_network(network_id).await.unwrap();
        assert_eq!(network.name, "Test Network");
        assert_eq!(network.subnet, "10.0.0.0/24");
        assert_eq!(network.gateway, "10.0.0.1");
    }

    #[tokio::test]
    async fn test_get_network_by_name() {
        let (store, _network_id) = create_test_store(test_database_path!()).await;

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
        let (store, _network_id) = create_test_store(test_database_path!()).await;

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
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Insert a lease that expired 1 hour ago
        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:01".to_string(), "10.0.0.101".to_string(), expired_time.clone(), expired_time, "active".to_string(), network_id),
            )
            .await
            .unwrap();

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 1);

        // Verify the lease is gone
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_expired_leases_preserves_active() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Insert a lease that expires in 1 hour
        let future_time = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let now = Utc::now().to_rfc3339();
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:02".to_string(), "10.0.0.102".to_string(), now, future_time, "active".to_string(), network_id),
            )
            .await
            .unwrap();

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
        let (store, network_id) = create_test_store(test_database_path!()).await;

        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let future_time = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let now = Utc::now().to_rfc3339();

        // Insert 2 expired leases
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:03".to_string(), "10.0.0.103".to_string(), expired_time.clone(), expired_time.clone(), "active".to_string(), network_id),
            )
            .await
            .unwrap();
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:04".to_string(), "10.0.0.104".to_string(), expired_time.clone(), expired_time, "offered".to_string(), network_id),
            )
            .await
            .unwrap();
        // Insert 1 active lease
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:05".to_string(), "10.0.0.105".to_string(), now, future_time, "active".to_string(), network_id),
            )
            .await
            .unwrap();

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
        let (store, _network_id) = create_test_store(test_database_path!()).await;

        // Delete expired leases when there are none
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_delete_expired_leases_released_and_expired() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Insert a released lease that is also expired
        let expired_time = (Utc::now() - Duration::hours(1)).to_rfc3339();
        store.db
            .execute(
                "INSERT INTO dhcp_leases (mac_address, ip_address, lease_start, lease_end, state, network_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ("aa:bb:cc:dd:ee:06".to_string(), "10.0.0.106".to_string(), expired_time.clone(), expired_time, "released".to_string(), network_id),
            )
            .await
            .unwrap();

        // Delete expired leases
        let deleted = store.delete_expired_leases().await.unwrap();
        assert_eq!(deleted, 1);

        // Verify the lease is gone
        let leases = store.get_leases_by_network(network_id).await.unwrap();
        assert_eq!(leases.len(), 0);
    }

    #[tokio::test]
    async fn test_create_or_update_static_reservation_insert() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Create a new reservation
        store
            .create_or_update_static_reservation(
                network_id,
                "aa:bb:cc:dd:ee:01",
                "10.0.0.50",
                Some("test-host"),
            )
            .await
            .unwrap();

        // Verify it was created
        let reservation = store
            .get_static_reservation(network_id, "aa:bb:cc:dd:ee:01")
            .await
            .unwrap();
        assert!(reservation.is_some());
        let r = reservation.unwrap();
        assert_eq!(r.mac_address, "aa:bb:cc:dd:ee:01");
        assert_eq!(r.ip_address, "10.0.0.50");
        assert_eq!(r.hostname, Some("test-host".to_string()));
    }

    #[tokio::test]
    async fn test_create_or_update_static_reservation_update() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Create initial reservation
        store
            .create_or_update_static_reservation(
                network_id,
                "aa:bb:cc:dd:ee:02",
                "10.0.0.51",
                Some("initial-host"),
            )
            .await
            .unwrap();

        // Update with different IP and hostname
        store
            .create_or_update_static_reservation(
                network_id,
                "aa:bb:cc:dd:ee:02",
                "10.0.0.52",
                Some("updated-host"),
            )
            .await
            .unwrap();

        // Verify it was updated, not duplicated
        let reservations = store.list_static_reservations(network_id).await.unwrap();
        let matching: Vec<_> = reservations
            .iter()
            .filter(|r| r.mac_address == "aa:bb:cc:dd:ee:02")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].ip_address, "10.0.0.52");
        assert_eq!(matching[0].hostname, Some("updated-host".to_string()));
    }

    #[tokio::test]
    async fn test_create_or_update_static_reservation_ip_conflict() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Create reservation for first MAC
        store
            .create_or_update_static_reservation(network_id, "aa:bb:cc:dd:ee:03", "10.0.0.53", None)
            .await
            .unwrap();

        // Try to create reservation for different MAC with same IP
        let result = store
            .create_or_update_static_reservation(network_id, "aa:bb:cc:dd:ee:04", "10.0.0.53", None)
            .await;

        // Should fail due to UNIQUE constraint on (network_id, ip_address)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_static_reservations_by_mac_single_network() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Create reservation
        store
            .create_or_update_static_reservation(network_id, "aa:bb:cc:dd:ee:05", "10.0.0.54", None)
            .await
            .unwrap();

        // Delete by MAC
        let deleted = store
            .delete_static_reservations_by_mac("aa:bb:cc:dd:ee:05")
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        // Verify it's gone
        let reservation = store
            .get_static_reservation(network_id, "aa:bb:cc:dd:ee:05")
            .await
            .unwrap();
        assert!(reservation.is_none());
    }

    #[tokio::test]
    async fn test_delete_static_reservations_by_mac_multiple_networks() {
        let (store, network_id) = create_test_store(test_database_path!()).await;

        // Create second network
        let network2 = store
            .create_network(
                "Second Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                None,
                false,
            )
            .await
            .unwrap();

        // Create reservations on both networks for same MAC
        store
            .create_or_update_static_reservation(network_id, "aa:bb:cc:dd:ee:06", "10.0.0.55", None)
            .await
            .unwrap();
        store
            .create_or_update_static_reservation(
                network2.id,
                "aa:bb:cc:dd:ee:06",
                "192.168.1.55",
                None,
            )
            .await
            .unwrap();

        // Delete by MAC - should remove from both networks
        let deleted = store
            .delete_static_reservations_by_mac("aa:bb:cc:dd:ee:06")
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        // Verify both are gone
        let r1 = store
            .get_static_reservation(network_id, "aa:bb:cc:dd:ee:06")
            .await
            .unwrap();
        assert!(r1.is_none());
        let r2 = store
            .get_static_reservation(network2.id, "aa:bb:cc:dd:ee:06")
            .await
            .unwrap();
        assert!(r2.is_none());
    }

    #[tokio::test]
    async fn test_delete_static_reservations_by_mac_nonexistent() {
        let (store, _network_id) = create_test_store(test_database_path!()).await;

        // Delete nonexistent MAC - should return 0, not error
        let deleted = store
            .delete_static_reservations_by_mac("aa:bb:cc:dd:ee:99")
            .await
            .unwrap();
        assert_eq!(deleted, 0);
    }
}
