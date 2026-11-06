//! Cloud-config UI endpoints: the global layer and a per-device preview of the merged seed.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::validation::{ValidationErrors, validate_cloud_config};
use crate::http::{AppState, error::Error as HttpError};
use crate::patch::Patch;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/ui/settings/cloud-config",
            get(get_global_cloud_config).put(put_global_cloud_config),
        )
        .route(
            "/ui/devices/{uuid}/cloud-config",
            get(get_device_cloud_config),
        )
        .with_state(state)
}

/// The global cloud-config layer, merged under every role's layer.
#[derive(Debug, Serialize)]
struct GlobalCloudConfig {
    cloud_config: Option<String>,
    updated_at: Option<DateTime<Utc>>,
}

impl From<Option<crate::settings::Setting>> for GlobalCloudConfig {
    fn from(setting: Option<crate::settings::Setting>) -> Self {
        Self {
            updated_at: setting.as_ref().map(|s| s.updated_at),
            cloud_config: setting.map(|s| s.value),
        }
    }
}

/// `PUT` body. `cloud_config` must be present: `null` clears the layer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UpdateGlobalCloudConfig {
    cloud_config: Patch<String>,
}

async fn get_global_cloud_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GlobalCloudConfig>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let setting = crate::settings::store::get_cloud_config(&conn).await?;
    Ok(Json(setting.into()))
}

async fn put_global_cloud_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateGlobalCloudConfig>,
) -> Result<Json<GlobalCloudConfig>, HttpError> {
    validate_update(&req).map_err(HttpError::ValidationError)?;
    let conn = state.connection_factory.open().await?;
    let value = req.cloud_config.value().map(String::as_str);
    let setting = crate::settings::store::set_cloud_config(&conn, value).await?;
    Ok(Json(setting.into()))
}

fn validate_update(
    req: &UpdateGlobalCloudConfig,
) -> Result<(), std::collections::HashMap<String, String>> {
    let mut errors = ValidationErrors::new();
    if req.cloud_config.is_keep() {
        errors.add_if_err(
            "cloud_config",
            Some("cloud_config is required; send null to clear".to_string()),
        );
    }
    errors.add_if_err(
        "cloud_config",
        validate_cloud_config(req.cloud_config.value().map(String::as_str)),
    );
    errors.into_result()
}

/// What a device would be served on its first boot.
#[derive(Debug, Serialize)]
struct DeviceCloudConfigPreview {
    /// Whether the device's role OS opts into cloud-init; when false nothing is served.
    supported: bool,
    /// Seed path relative to the rack-director root URL (the NoCloud `seedfrom` base).
    seed_path: String,
    user_data: Option<String>,
    meta_data: Option<String>,
}

/// Preview a device's merged cloud-config seed (not limited to its first boot).
async fn get_device_cloud_config(
    State(state): State<Arc<AppState>>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<DeviceCloudConfigPreview>, HttpError> {
    let mut conn = state.connection_factory.open().await?;
    let plan_id = crate::plans::store::get_active_plan_for_device(&conn, &uuid)
        .await?
        .and_then(|plan| plan.id);
    let seed = crate::http::cnc::cloud_init::build_device_seed(&mut conn, &uuid, plan_id).await?;
    Ok(Json(DeviceCloudConfigPreview {
        supported: seed.is_some(),
        seed_path: crate::cloud_init::seed_path(&uuid),
        user_data: seed.as_ref().map(|s| s.user_data.clone()),
        meta_data: seed.map(|s| s.meta_data),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_helpers::build_test_state;
    use crate::{database, test_connection_factory};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    async fn send(
        state: Arc<AppState>,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(match body {
                Some(b) => Body::from(b.to_string()),
                None => Body::empty(),
            })
            .unwrap();
        let response = routes(state).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// `factory` must come from `test_connection_factory!()` at the test's own call site.
    async fn setup(
        factory: database::DatabaseConnectionFactory,
    ) -> (Arc<AppState>, database::Connection) {
        let conn = database::run_migrations(&factory).await.unwrap();
        (build_test_state(Arc::new(factory)), conn)
    }

    #[tokio::test]
    async fn test_global_cloud_config_set_get_clear() {
        let (state, _conn) = setup(test_connection_factory!()).await;
        let uri = "/ui/settings/cloud-config";

        let (status, body) = send(state.clone(), "GET", uri, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["cloud_config"], serde_json::Value::Null);

        let text = "#cloud-config\npackages: [vim]\n";
        let (status, body) = send(
            state.clone(),
            "PUT",
            uri,
            Some(serde_json::json!({ "cloud_config": text })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["cloud_config"], text);
        assert!(body["updated_at"].is_string());

        let (_, body) = send(state.clone(), "GET", uri, None).await;
        assert_eq!(body["cloud_config"], text);

        let (status, body) = send(
            state.clone(),
            "PUT",
            uri,
            Some(serde_json::json!({ "cloud_config": null })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["cloud_config"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_global_cloud_config_rejects_invalid_and_missing() {
        let (state, _conn) = setup(test_connection_factory!()).await;
        let uri = "/ui/settings/cloud-config";

        let (status, body) = send(
            state.clone(),
            "PUT",
            uri,
            Some(serde_json::json!({ "cloud_config": "- a\n- b\n" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["errors"]["cloud_config"].is_string(), "{body}");

        let (status, body) = send(state, "PUT", uri, Some(serde_json::json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["errors"]["cloud_config"]
                .as_str()
                .unwrap()
                .contains("required")
        );
    }

    #[tokio::test]
    async fn test_device_preview_without_role_is_bad_request() {
        let (state, mut conn) = setup(test_connection_factory!()).await;
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440401").unwrap();
        crate::director::Director::new(&mut conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let (status, _) = send(
            state,
            "GET",
            &format!("/ui/devices/{uuid}/cloud-config"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
