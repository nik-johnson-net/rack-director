use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct DhcpStore {
    db: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub id: i64,
    pub mac_address: String,
    pub ip_address: String,
    pub device_uuid: Option<String>,
    pub lease_start: DateTime<Utc>,
    pub lease_end: DateTime<Utc>,
    pub state: LeaseState,
    pub hostname: Option<String>,
    pub network_id: Option<i64>,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

impl DhcpStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Create or update a DHCP lease with network context
    pub async fn create_or_update_lease_with_network(
        &self,
        mac: &str,
        ip: &Ipv4Addr,
        device_uuid: Option<&str>,
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

        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE mac_address = ?",
        )?;

        let lease = stmt
            .query_row(params![mac], |row| {
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
            })
            .optional()?;

        Ok(lease)
    }

    /// Get lease by ID
    pub async fn get_lease_by_id(&self, id: i64) -> Result<Option<Lease>> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE id = ?",
        )?;

        let lease = stmt
            .query_row(params![id], |row| {
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
            })
            .optional()?;

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
        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases ORDER BY updated_at DESC",
        )?;

        let leases = stmt
            .query_map([], |row| {
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

    /// Find lease by device UUID (synchronous for use in non-async contexts)
    pub fn find_lease_by_device_uuid(&self, device_uuid: &str) -> Result<Option<Lease>> {
        // Get the db without using async
        let db = self
            .db
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Could not lock database"))?;

        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname, network_id
             FROM dhcp_leases WHERE device_uuid = ? AND state = 'active' ORDER BY lease_end DESC LIMIT 1",
        )?;

        let lease = stmt
            .query_row(params![device_uuid], |row| {
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
            })
            .optional()?;

        Ok(lease)
    }

    // ========== Network CRUD Operations ==========

    /// Get a network by ID
    pub async fn get_network(&self, id: i64) -> Result<DhcpNetwork> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
             FROM dhcp_networks WHERE id = ?",
        )?;

        let network = stmt.query_row(params![id], |row| {
            let dns_servers_json: String = row.get(4)?;
            let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

            Ok(DhcpNetwork {
                id: row.get(0)?,
                name: row.get(1)?,
                subnet: row.get(2)?,
                gateway: row.get(3)?,
                dns_servers,
                lease_duration: row.get(5)?,
                relay_agent_address: row.get(6)?,
                created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
            })
        })?;

        Ok(network)
    }

    /// Get a network by relay agent address (or None for local L2)
    pub async fn get_network_by_relay(
        &self,
        relay: Option<Ipv4Addr>,
    ) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;
        let relay_str = relay.map(|r| r.to_string());

        let mut stmt = db.prepare(
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
             FROM dhcp_networks WHERE relay_agent_address IS ? OR (relay_agent_address IS NULL AND ? IS NULL)",
        )?;

        let network = stmt
            .query_row(params![relay_str, relay_str], |row| {
                let dns_servers_json: String = row.get(4)?;
                let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                    .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

                Ok(DhcpNetwork {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subnet: row.get(2)?,
                    gateway: row.get(3)?,
                    dns_servers,
                    lease_duration: row.get(5)?,
                    relay_agent_address: row.get(6)?,
                    created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
                })
            })
            .optional()?;

        Ok(network)
    }

    /// Get a network by name
    pub async fn get_network_by_name(&self, name: &str) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare(
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
             FROM dhcp_networks WHERE name = ?",
        )?;

        let network = stmt
            .query_row(params![name], |row| {
                let dns_servers_json: String = row.get(4)?;
                let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                    .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

                Ok(DhcpNetwork {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subnet: row.get(2)?,
                    gateway: row.get(3)?,
                    dns_servers,
                    lease_duration: row.get(5)?,
                    relay_agent_address: row.get(6)?,
                    created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
                })
            })
            .optional()?;

        Ok(network)
    }

    /// Get a network by relay agent address string (checking both NULL and empty string for Default L2)
    pub async fn get_network_by_relay_string(
        &self,
        relay_agent_address: Option<&str>,
    ) -> Result<Option<DhcpNetwork>> {
        let db = self.db.lock().await;

        // Handle the three cases:
        // 1. None or Some("") - Default L2 network (NULL or empty string)
        // 2. Some(address) - Specific relay agent address
        let network = match relay_agent_address {
            None | Some("") => {
                let mut stmt = db.prepare(
                    "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
                     FROM dhcp_networks WHERE relay_agent_address IS NULL OR relay_agent_address = ''",
                )?;

                stmt.query_row([], |row| {
                    let dns_servers_json: String = row.get(4)?;
                    let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                        .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

                    Ok(DhcpNetwork {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        subnet: row.get(2)?,
                        gateway: row.get(3)?,
                        dns_servers,
                        lease_duration: row.get(5)?,
                        relay_agent_address: row.get(6)?,
                        created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                        updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
                    })
                })
                .optional()?
            }
            Some(addr) => {
                let addr_string = addr.to_string();
                let mut stmt = db.prepare(
                    "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
                     FROM dhcp_networks WHERE relay_agent_address = ?",
                )?;

                stmt.query_row(params![addr_string], |row| {
                    let dns_servers_json: String = row.get(4)?;
                    let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                        .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

                    Ok(DhcpNetwork {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        subnet: row.get(2)?,
                        gateway: row.get(3)?,
                        dns_servers,
                        lease_duration: row.get(5)?,
                        relay_agent_address: row.get(6)?,
                        created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                        updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
                    })
                })
                .optional()?
            }
        };

        Ok(network)
    }

    /// List all networks
    pub async fn list_networks(&self) -> Result<Vec<DhcpNetwork>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at
             FROM dhcp_networks ORDER BY name",
        )?;

        let networks = stmt
            .query_map([], |row| {
                let dns_servers_json: String = row.get(4)?;
                let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                    .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

                Ok(DhcpNetwork {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    subnet: row.get(2)?,
                    gateway: row.get(3)?,
                    dns_servers,
                    lease_duration: row.get(5)?,
                    relay_agent_address: row.get(6)?,
                    created_at: parse_datetime(&row.get::<_, String>(7)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(8)?).unwrap(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(networks)
    }

    /// Create a new network
    pub async fn create_network(
        &self,
        name: &str,
        subnet: &str,
        gateway: &str,
        dns_servers: &[String],
        lease_duration: u32,
        relay_agent_address: Option<&str>,
    ) -> Result<DhcpNetwork> {
        let dns_servers_json = serde_json::to_string(dns_servers)?;
        let now = Utc::now().to_rfc3339();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_networks (name, subnet, gateway, dns_servers, lease_duration, relay_agent_address, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![name, subnet, gateway, dns_servers_json, lease_duration, relay_agent_address, now, now],
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
        let mut stmt = db.prepare(
            "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
             FROM dhcp_pools WHERE id = ?",
        )?;

        let pool = stmt.query_row(params![id], |row| {
            Ok(DhcpPool {
                id: row.get(0)?,
                network_id: row.get(1)?,
                name: row.get(2)?,
                range_start: row.get(3)?,
                range_end: row.get(4)?,
                created_at: parse_datetime(&row.get::<_, String>(5)?).unwrap(),
                updated_at: parse_datetime(&row.get::<_, String>(6)?).unwrap(),
            })
        })?;

        Ok(pool)
    }

    /// List all pools for a network
    pub async fn list_pools_for_network(&self, network_id: i64) -> Result<Vec<DhcpPool>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, network_id, name, range_start, range_end, created_at, updated_at
             FROM dhcp_pools WHERE network_id = ? ORDER BY name",
        )?;

        let pools = stmt
            .query_map(params![network_id], |row| {
                Ok(DhcpPool {
                    id: row.get(0)?,
                    network_id: row.get(1)?,
                    name: row.get(2)?,
                    range_start: row.get(3)?,
                    range_end: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(6)?).unwrap(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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
        let mut stmt = db.prepare(
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE network_id = ? AND mac_address = ?",
        )?;

        let reservation = stmt
            .query_row(params![network_id, mac], |row| {
                Ok(StaticReservation {
                    id: row.get(0)?,
                    network_id: row.get(1)?,
                    mac_address: row.get(2)?,
                    ip_address: row.get(3)?,
                    hostname: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(6)?).unwrap(),
                })
            })
            .optional()?;

        Ok(reservation)
    }

    /// List all static reservations for a network
    pub async fn list_static_reservations(
        &self,
        network_id: i64,
    ) -> Result<Vec<StaticReservation>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE network_id = ? ORDER BY ip_address",
        )?;

        let reservations = stmt
            .query_map(params![network_id], |row| {
                Ok(StaticReservation {
                    id: row.get(0)?,
                    network_id: row.get(1)?,
                    mac_address: row.get(2)?,
                    ip_address: row.get(3)?,
                    hostname: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?).unwrap(),
                    updated_at: parse_datetime(&row.get::<_, String>(6)?).unwrap(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

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
        drop(db);

        // Fetch and return the created reservation
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, network_id, mac_address, ip_address, hostname, created_at, updated_at
             FROM dhcp_static_reservations WHERE id = ?",
        )?;

        let reservation = stmt.query_row(params![id], |row| {
            Ok(StaticReservation {
                id: row.get(0)?,
                network_id: row.get(1)?,
                mac_address: row.get(2)?,
                ip_address: row.get(3)?,
                hostname: row.get(4)?,
                created_at: parse_datetime(&row.get::<_, String>(5)?).unwrap(),
                updated_at: parse_datetime(&row.get::<_, String>(6)?).unwrap(),
            })
        })?;

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

    async fn create_test_store() -> (DhcpStore, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(db_path).unwrap();
        (DhcpStore::new(Arc::new(Mutex::new(conn))), temp_dir)
    }

    #[tokio::test]
    async fn test_get_default_network() {
        let (store, _temp_dir) = create_test_store().await;
        let network = store.get_network(1).await.unwrap();
        assert_eq!(network.name, "Default");
        assert_eq!(network.subnet, "10.0.0.0/24");
        assert_eq!(network.gateway, "10.0.0.1");
    }

    #[tokio::test]
    async fn test_get_network_by_name() {
        let (store, _temp_dir) = create_test_store().await;

        // Test existing network
        let network = store.get_network_by_name("Default").await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Default");

        // Test non-existent network
        let network = store.get_network_by_name("NonExistent").await.unwrap();
        assert!(network.is_none());
    }

    #[tokio::test]
    async fn test_get_network_by_relay_string() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network with a relay agent
        store
            .create_network(
                "Relay Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
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

        // Test finding Default L2 network (None)
        let network = store.get_network_by_relay_string(None).await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Default");
        assert!(network.relay_agent_address.is_none());

        // Test finding Default L2 network (empty string)
        let network = store.get_network_by_relay_string(Some("")).await.unwrap();
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.name, "Default");

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
}
