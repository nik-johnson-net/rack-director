//! NoCloud seed endpoint: `GET /cnc/devices/{uuid}/cloud-init/{file}`.
//!
//! A provisioned OS's cloud-init fetches `meta-data` and `user-data` from here on first boot
//! (the install script points its NoCloud `seedfrom` at this path).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{StatusCode, header},
    response::Response,
};
use uuid::Uuid;

use crate::cloud_init::{CloudInitSeed, SeedDevice};
use crate::http::{AppState, error::Error};
use crate::{database, director::Director, plans::Action};

const META_DATA: &str = "meta-data";
const USER_DATA: &str = "user-data";

/// Serve one file of a device's NoCloud seed.
///
/// Served only while the device's active plan is at `first_boot`: cloud-config can carry
/// secrets and this endpoint is unauthenticated, while installed hosts fetch it exactly once
/// (`manual_cache_clean`). `vendor-data` and `network-config` are optional to cloud-init and
/// are never served.
pub(super) async fn seed_file_handler(
    State(state): State<Arc<AppState>>,
    Path((uuid, file)): Path<(Uuid, String)>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
) -> Result<Response<String>, Error> {
    let select: fn(CloudInitSeed) -> String = match file.as_str() {
        META_DATA => |seed| seed.meta_data,
        USER_DATA => |seed| seed.user_data,
        _ => return Err(Error::NotFound(format!("No cloud-init {file} is served"))),
    };

    let mut conn = state
        .connection_factory
        .open()
        .await
        .map_err(Error::ServerInternalError)?;
    let plan = crate::plans::store::get_active_plan_for_device(&conn, &uuid)
        .await
        .map_err(Error::ServerInternalError)?;
    let at_first_boot = plan
        .as_ref()
        .and_then(|p| p.get_current_action())
        .is_some_and(|action| *action == Action::FirstBoot);
    if !at_first_boot {
        log::warn!(
            "Refusing cloud-init {file} for {uuid} to {remote}: device is not at first_boot"
        );
        return Err(Error::NotFound(format!(
            "Device {uuid} is not at its first boot"
        )));
    }

    let seed = build_device_seed(&mut conn, &uuid, plan.and_then(|p| p.id))
        .await?
        .ok_or_else(|| Error::NotFound(format!("Device {uuid}'s OS does not use cloud-init")))?;
    log::info!("Serving cloud-init {file} to device {uuid} ({remote})");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(select(seed))
        .expect("response building should never error"))
}

/// Build a device's seed from the global and role cloud-config layers.
///
/// Returns `Ok(None)` when the role's OS does not opt into cloud-init. Errors with
/// `NotFound` for an unknown device, `BadRequest` when the device has no role, and
/// `ServerInternalError` when the database fails, so a transient error isn't mistaken for a
/// missing device.
pub(crate) async fn build_device_seed(
    conn: &mut database::Connection,
    uuid: &Uuid,
    plan_id: Option<i64>,
) -> Result<Option<CloudInitSeed>, Error> {
    let director = Director::new(conn);
    if !director
        .device_exists(uuid)
        .await
        .map_err(Error::ServerInternalError)?
    {
        return Err(Error::NotFound(format!("Device {uuid} not found")));
    }
    let device = director
        .get_device(uuid)
        .await
        .map_err(Error::ServerInternalError)?;
    let role_id = device
        .role_id
        .ok_or_else(|| Error::BadRequest("Device has no role assigned".to_string()))?;
    let role = crate::roles::store::get(conn, role_id)
        .await
        .map_err(Error::ServerInternalError)?;
    let resolved = role
        .resolve_os(conn)
        .await
        .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Failed to resolve OS: {e}")))?;
    if !resolved.os.config.cloud_init {
        return Ok(None);
    }

    let global = crate::settings::store::get_cloud_config(conn)
        .await
        .map_err(Error::ServerInternalError)?;
    let fqdn = super::install_script::device_fqdn(conn, &device).await?;
    let seed = crate::cloud_init::build_seed(
        global.as_ref().map(|s| s.value.as_str()),
        role.cloud_config.as_deref(),
        &SeedDevice {
            uuid,
            fqdn: &fqdn,
            plan_id,
        },
    )
    .map_err(Error::ServerInternalError)?;
    Ok(Some(seed))
}

#[cfg(test)]
mod tests {
    use super::super::{routes, test_helpers::setup_test_state};
    use super::*;
    use crate::director::test_support::{provisionable_device, start_provision};
    use crate::test_connection_factory;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    fn uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440301").unwrap()
    }

    /// Register a device whose role runs an OS with (or without) cloud-init, set a global
    /// and role cloud-config, and start provisioning. Returns the plan id.
    async fn provisioning_device(conn: &mut database::Connection, cloud_init: bool) -> i64 {
        // Provisioning refuses a role cloud-config on an OS without cloud-init.
        let role_cloud_config = cloud_init.then_some(
            "packages: [jq]
",
        );
        provisionable_device(conn, &uuid(), cloud_init, role_cloud_config).await;
        crate::settings::store::set_cloud_config(
            conn,
            Some(
                "disable_root: false
users:
  - name: root
    ssh_authorized_keys: [k1]
",
            ),
        )
        .await
        .unwrap();
        start_provision(conn, &uuid()).await.id.unwrap()
    }

    /// Advance the device's plan past partition_disks and install_os to first_boot.
    async fn advance_to_first_boot(conn: &mut database::Connection, plan_id: i64) {
        let mut director = Director::new(conn);
        director
            .mark_action_success(&uuid(), crate::director::ReportedStep::plan(Some(plan_id)))
            .await
            .unwrap();
        director
            .mark_action_success(&uuid(), crate::director::ReportedStep::plan(Some(plan_id)))
            .await
            .unwrap();
    }

    async fn get(state: Arc<AppState>, file: &str) -> (StatusCode, String) {
        let app = routes(state).layer(axum::extract::connect_info::MockConnectInfo(
            SocketAddr::from(([10, 0, 0, 50], 40000)),
        ));
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/cnc/devices/{}/cloud-init/{file}", uuid()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn test_serves_merged_seed_at_first_boot() {
        let (state, _dir, mut conn) = setup_test_state(test_connection_factory!()).await;
        let plan_id = provisioning_device(&mut conn, true).await;
        advance_to_first_boot(&mut conn, plan_id).await;

        let (status, user_data) = get(state.clone(), "user-data").await;
        assert_eq!(status, StatusCode::OK);
        assert!(user_data.starts_with("#cloud-config\n"), "{user_data}");
        let doc: serde_json::Value = serde_saphyr::from_str(&user_data).unwrap();
        assert_eq!(
            doc["users"][0]["ssh_authorized_keys"],
            serde_json::json!(["k1"])
        );
        assert_eq!(doc["packages"], serde_json::json!(["jq"]));

        let (status, meta_data) = get(state, "meta-data").await;
        assert_eq!(status, StatusCode::OK);
        let meta: serde_json::Value = serde_saphyr::from_str(&meta_data).unwrap();
        assert_eq!(
            meta["instance-id"],
            serde_json::json!(format!("rd-{}-p{plan_id}", uuid()))
        );
    }

    /// Before first boot (and after provisioning) the seed is not exposed.
    #[tokio::test]
    async fn test_refuses_seed_outside_first_boot() {
        let (state, _dir, mut conn) = setup_test_state(test_connection_factory!()).await;
        provisioning_device(&mut conn, true).await; // still at partition_disks

        let (status, _) = get(state, "user-data").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_optional_seed_files_are_not_served() {
        let (state, _dir, mut conn) = setup_test_state(test_connection_factory!()).await;
        let plan_id = provisioning_device(&mut conn, true).await;
        advance_to_first_boot(&mut conn, plan_id).await;

        assert_eq!(
            get(state.clone(), "vendor-data").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(get(state, "network-config").await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_build_device_seed_none_without_cloud_init() {
        let (_state, _dir, mut conn) = setup_test_state(test_connection_factory!()).await;
        provisioning_device(&mut conn, false).await;

        let seed = build_device_seed(&mut conn, &uuid(), None).await.unwrap();
        assert_eq!(seed, None);
    }

    #[tokio::test]
    async fn test_build_device_seed_unknown_device_is_not_found() {
        let (_state, _dir, mut conn) = setup_test_state(test_connection_factory!()).await;
        let result = build_device_seed(&mut conn, &uuid(), None).await;
        assert!(matches!(result, Err(Error::NotFound(_))), "{result:?}");
    }
}
