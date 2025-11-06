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

#[derive(Debug, Clone)]
pub struct DhcpConfig {
    pub subnet: String,
    pub range_start: String,
    pub range_end: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub lease_duration: u32,
    pub tftp_server: String,
    pub http_server: String,
}

impl DhcpStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Load DHCP configuration from database
    pub async fn load_config(&self) -> Result<DhcpConfig> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare("SELECT subnet, range_start, range_end, gateway, dns_servers, lease_duration, tftp_server, http_server FROM dhcp_config WHERE id = 1")?;

        let config = stmt.query_row([], |row| {
            let dns_servers_json: String = row.get(4)?;
            let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

            Ok(DhcpConfig {
                subnet: row.get(0)?,
                range_start: row.get(1)?,
                range_end: row.get(2)?,
                gateway: row.get(3)?,
                dns_servers,
                lease_duration: row.get(5)?,
                tftp_server: row.get(6)?,
                http_server: row.get(7)?,
            })
        })?;

        Ok(config)
    }

    /// Create or update a DHCP lease
    pub async fn create_or_update_lease(
        &self,
        mac: &str,
        ip: &Ipv4Addr,
        device_uuid: Option<&str>,
        state: LeaseState,
        lease_duration: u32,
    ) -> Result<()> {
        let ip_str = ip.to_string();
        let now = Utc::now();
        let lease_end = now + Duration::seconds(lease_duration as i64);

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO dhcp_leases
                (mac_address, ip_address, device_uuid, lease_start, lease_end, state, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(mac_address) DO UPDATE SET
                ip_address = ?2,
                device_uuid = ?3,
                lease_start = ?4,
                lease_end = ?5,
                state = ?6,
                updated_at = ?7",
            params![
                mac,
                ip_str,
                device_uuid,
                now.to_rfc3339(),
                lease_end.to_rfc3339(),
                state.to_string(),
                now.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Get lease by MAC address
    pub async fn get_lease_by_mac(&self, mac: &str) -> Result<Option<Lease>> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname
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

    /// Get all active leases (not expired)
    pub async fn get_active_leases(&self) -> Result<Vec<Lease>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname
             FROM dhcp_leases WHERE state = 'active' AND lease_end > datetime('now')",
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(leases)
    }

    /// Get all leases (for API/management)
    pub async fn get_all_leases(&self) -> Result<Vec<Lease>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(leases)
    }

    /// Find lease by device UUID (synchronous for use in non-async contexts)
    pub fn find_lease_by_device_uuid(&self, device_uuid: &str) -> Result<Option<Lease>> {
        // Get the db without using async
        let db = self.db.try_lock()
            .map_err(|_| anyhow::anyhow!("Could not lock database"))?;

        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, device_uuid, lease_start, lease_end, state, hostname
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
                })
            })
            .optional()?;

        Ok(lease)
    }

    /// Get DHCP config (synchronous for use in non-async contexts)
    pub fn get_config(&self) -> Result<DhcpConfig> {
        let db = self.db.try_lock()
            .map_err(|_| anyhow::anyhow!("Could not lock database"))?;

        let mut stmt = db.prepare("SELECT subnet, range_start, range_end, gateway, dns_servers, lease_duration, tftp_server, http_server FROM dhcp_config WHERE id = 1")?;

        let config = stmt.query_row([], |row| {
            let dns_servers_json: String = row.get(4)?;
            let dns_servers: Vec<String> = serde_json::from_str(&dns_servers_json)
                .unwrap_or_else(|_| vec!["8.8.8.8".to_string()]);

            Ok(DhcpConfig {
                subnet: row.get(0)?,
                range_start: row.get(1)?,
                range_end: row.get(2)?,
                gateway: row.get(3)?,
                dns_servers,
                lease_duration: row.get(5)?,
                tftp_server: row.get(6)?,
                http_server: row.get(7)?,
            })
        })?;

        Ok(config)
    }
}

pub fn format_mac(mac: &[u8]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
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
    async fn test_load_config() {
        let (store, _temp_dir) = create_test_store().await;
        let config = store.load_config().await.unwrap();
        assert_eq!(config.subnet, "10.0.0.0/24");
        assert_eq!(config.gateway, "10.0.0.1");
    }

    #[tokio::test]
    async fn test_create_and_get_lease() {
        let (store, _temp_dir) = create_test_store().await;
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();

        store
            .create_or_update_lease(mac, &ip, None, LeaseState::Offered, 3600)
            .await
            .unwrap();

        let lease = store.get_lease_by_mac(mac).await.unwrap().unwrap();
        assert_eq!(lease.mac_address, mac);
        assert_eq!(lease.ip_address, "10.0.0.100");
        assert_eq!(lease.state, LeaseState::Offered);
    }

    #[tokio::test]
    async fn test_activate_lease() {
        let (store, _temp_dir) = create_test_store().await;
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();

        store
            .create_or_update_lease(mac, &ip, None, LeaseState::Offered, 3600)
            .await
            .unwrap();

        store.activate_lease(mac).await.unwrap();

        let lease = store.get_lease_by_mac(mac).await.unwrap().unwrap();
        assert_eq!(lease.state, LeaseState::Active);
    }

    #[tokio::test]
    async fn test_format_mac() {
        let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(format_mac(&mac), "aa:bb:cc:dd:ee:ff");
    }
}
