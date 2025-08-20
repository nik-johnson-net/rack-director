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
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle) VALUES (?1, 'new')",
            [uuid],
        )?;
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
}
