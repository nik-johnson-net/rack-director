//! Table access for device requests: the columns migration 27 added to `devices`.

use common::device_attributes::{BmcConfig, BmcInfo};
use serde_json::Value;
use uuid::Uuid;

use crate::database::Connection;
use crate::lifecycle::DeviceLifecycle;

/// What the device's recorded requests ask for, read from one narrow query. Idle daemons poll
/// every few seconds, so this avoids loading and parsing the whole attributes blob.
pub(super) struct RequestRow {
    pub(super) lifecycle: DeviceLifecycle,
    pub(super) rediscover_requested: bool,
    pub(super) bmc_config_attempted: Option<Value>,
    pub(super) last_request_plan_id: Option<i64>,
    pub(super) bmc: Option<BmcInfo>,
    pub(super) bmc_config: Option<BmcConfig>,
}

impl RequestRow {
    pub(super) async fn load(conn: &Connection, uuid: &Uuid) -> anyhow::Result<Self> {
        let row = conn
            .query_one(
                "SELECT lifecycle, rediscover_requested, bmc_config_attempted,
                        last_request_plan_id,
                        json_extract(attributes, '$.bmc'),
                        json_extract(attributes, '$.bmc_config')
                 FROM devices WHERE uuid = ?1",
                (*uuid,),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .await?;
        Ok(RequestRow {
            lifecycle: DeviceLifecycle::from(row.0),
            rediscover_requested: row.1,
            bmc_config_attempted: parse_json(row.2),
            last_request_plan_id: row.3,
            bmc: parse_json(row.4),
            bmc_config: parse_json(row.5),
        })
    }
}

/// A stored value that no longer parses is treated as absent rather than failing every boot.
fn parse_json<T: serde::de::DeserializeOwned>(json: Option<String>) -> Option<T> {
    json.and_then(|s| serde_json::from_str(&s).ok())
}

pub(super) async fn set_rediscover_requested(conn: &Connection, uuid: &Uuid) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE devices SET rediscover_requested = 1 WHERE uuid = ?1",
        (*uuid,),
    )
    .await?;
    Ok(())
}

pub(super) async fn clear_bmc_config_attempted(
    conn: &Connection,
    uuid: &Uuid,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE devices SET bmc_config_attempted = NULL WHERE uuid = ?1",
        (*uuid,),
    )
    .await?;
    Ok(())
}

/// Record that `plan_id` was created for the device's requests: rediscovery is satisfied, and
/// `bmc_target` (when the plan configures the BMC) becomes the attempted BMC settings.
pub(super) async fn consume_requests(
    conn: &Connection,
    uuid: &Uuid,
    bmc_target: Option<&Value>,
    plan_id: i64,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE devices SET rediscover_requested = 0,
                bmc_config_attempted = COALESCE(?1, bmc_config_attempted),
                last_request_plan_id = ?2
         WHERE uuid = ?3",
        (bmc_target.map(Value::to_string), plan_id, *uuid),
    )
    .await?;
    Ok(())
}
