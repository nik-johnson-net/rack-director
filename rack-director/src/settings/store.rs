use anyhow::Result;
use chrono::Utc;
use rusqlite::OptionalExtension;

use super::Setting;
use crate::database::Connection;

/// Key of the global cloud-config layer (YAML text).
const CLOUD_CONFIG_KEY: &str = "cloud_config";

/// The global cloud-config layer, if one is set.
pub async fn get_cloud_config(conn: &Connection) -> Result<Option<Setting>> {
    get(conn, CLOUD_CONFIG_KEY).await
}

/// Set (`Some`) or clear (`None`) the global cloud-config layer. Returns the stored value.
pub async fn set_cloud_config(conn: &Connection, value: Option<&str>) -> Result<Option<Setting>> {
    match value {
        Some(value) => set(conn, CLOUD_CONFIG_KEY, value).await.map(Some),
        None => {
            delete(conn, CLOUD_CONFIG_KEY).await?;
            Ok(None)
        }
    }
}

async fn get(conn: &Connection, key: &'static str) -> Result<Option<Setting>> {
    let setting = conn
        .query_row(
            "SELECT value, updated_at FROM settings WHERE key = ?1",
            (key,),
            |row| {
                Ok(Setting {
                    value: row.get(0)?,
                    updated_at: row.get(1)?,
                })
            },
        )
        .await
        .optional()?;
    Ok(setting)
}

async fn set(conn: &Connection, key: &'static str, value: &str) -> Result<Setting> {
    let updated_at = Utc::now();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        (key, value.to_string(), updated_at),
    )
    .await?;
    Ok(Setting {
        value: value.to_string(),
        updated_at,
    })
}

async fn delete(conn: &Connection, key: &'static str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", (key,))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, test_connection_factory};

    #[tokio::test]
    async fn test_cloud_config_unset_by_default() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();
        assert_eq!(get_cloud_config(&conn).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_set_replace_and_clear_cloud_config() {
        let conn = database::run_migrations(&test_connection_factory!())
            .await
            .unwrap();

        set_cloud_config(&conn, Some("packages: [vim]\n"))
            .await
            .unwrap();
        let stored = set_cloud_config(&conn, Some("packages: [jq]\n"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.value, "packages: [jq]\n");

        let fetched = get_cloud_config(&conn).await.unwrap().unwrap();
        assert_eq!(fetched.value, "packages: [jq]\n");

        assert_eq!(set_cloud_config(&conn, None).await.unwrap(), None);
        assert_eq!(get_cloud_config(&conn).await.unwrap(), None);
    }
}
