use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct DirectorStore {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
}

impl DirectorStore {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { conn }
    }

    pub async fn register_device(&self, uuid: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("INSERT INTO devices (uuid) VALUES (?1)", [uuid])?;
        Ok(())
    }

    pub async fn update_device_last_seen(&self, uuid: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE devices SET last_seen_at = CURRENT_TIMESTAMP WHERE uuid = ?1",
            [uuid],
        )?;
        Ok(())
    }

    pub async fn update_attributes(
        &self,
        uuid: &str,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE devices SET attributes = ?1 WHERE uuid = ?2",
            [&serde_json::to_string(&attributes)?, uuid],
        )?;
        Ok(())
    }

    pub async fn get_all_devices(
        &self,
    ) -> Result<Vec<(String, Option<serde_json::Map<String, serde_json::Value>>)>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare("SELECT uuid, attributes FROM devices")?;
        let rows = stmt.query_map([], |row| {
            let uuid: String = row.get(0)?;
            let attributes_json: Option<String> = row.get(1)?;
            let attributes = match attributes_json {
                Some(json_str) => {
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                        &json_str,
                    ).ok()
                }
                None => None,
            };
            Ok((uuid, attributes))
        })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row?);
        }

        Ok(devices)
    }
}
