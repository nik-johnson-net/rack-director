use uuid::Uuid;

use crate::database::{Connection, FromRow};
use crate::director::store::Device;

/// Get a device by UUID.
pub async fn get_device_by_uuid(conn: &Connection, uuid: &Uuid) -> rusqlite::Result<Device> {
    conn.query_one(
        "SELECT uuid, architecture, lifecycle, role_id, platform_id, attributes, created_at, first_seen_at, last_seen_at FROM devices WHERE uuid = ?1",
        (*uuid,),
        Device::from_row,
    )
    .await
}
