use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct DirectorStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
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
}
