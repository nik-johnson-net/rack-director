use std::io::Write as _;
use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, post},
};
use futures::StreamExt;
use serde::Serialize;

use crate::http::AppState;
use crate::http::error::Error as HttpError;
use crate::osm::store;

/// Maximum OSM archive upload size: 10 GiB.
const MAX_OSM_UPLOAD_SIZE: usize = 10 * 1024 * 1024 * 1024;

/// Progress update interval: every ~1 MiB of received data.
const PROGRESS_UPDATE_INTERVAL_BYTES: i64 = 1024 * 1024;

// ── Response types ────────────────────────────────────────────────────────────

/// A module with a count of its associated OS entries, returned by the list and
/// get endpoints.
#[derive(Debug, Serialize)]
pub struct OsmModuleResponse {
    #[serde(flatten)]
    pub module: store::OsmModule,
    pub os_count: usize,
}

// ── Route registration ────────────────────────────────────────────────────────

/// Register all OSM HTTP routes under `/ui/osm/`.
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/osm/modules", get(list_modules))
        .route("/ui/osm/modules/{id}", get(get_module))
        .route("/ui/osm/modules/{id}", delete(delete_module))
        .route(
            "/ui/osm/modules/{id}/operating-systems",
            get(list_module_os),
        )
        .route("/ui/osm/modules/{id}/export", get(export_module))
        .route(
            "/ui/osm/upload",
            post(upload_osm).layer(DefaultBodyLimit::max(MAX_OSM_UPLOAD_SIZE)),
        )
        .route("/ui/osm/uploads", get(list_uploads))
        .route("/ui/osm/uploads/{id}", get(get_upload))
        .route("/ui/osm/operating-systems", get(list_all_os))
        .route("/ui/osm/operating-systems/{id}/disable", post(disable_os))
        .route("/ui/osm/operating-systems/{id}/enable", post(enable_os))
        .with_state(state)
}

// ── Module handlers ───────────────────────────────────────────────────────────

/// List all OSM modules, including the count of OS entries for each.
async fn list_modules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<OsmModuleResponse>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let modules = store::list_modules(&conn).await?;

    let mut responses = Vec::with_capacity(modules.len());
    for module in modules {
        let os_count = store::list_operating_systems(&conn, module.id).await?.len();
        responses.push(OsmModuleResponse { module, os_count });
    }

    Ok(Json(responses))
}

/// Get a single OSM module by ID, including its OS count.
///
/// Returns 404 if no module with the given ID exists.
async fn get_module(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<OsmModuleResponse>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let module = store::get_module(&conn, id)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM module {id} not found")))?;
    let os_count = store::list_operating_systems(&conn, module.id).await?.len();
    Ok(Json(OsmModuleResponse { module, os_count }))
}

/// Delete an OSM module by ID.
///
/// Returns 400 if the caller attempts to delete the built-in "default" module,
/// and 404 if no module with the given ID exists. Returns 204 on success.
async fn delete_module(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    let module = store::get_module(&conn, id)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM module {id} not found")))?;

    if module.is_default {
        return Err(HttpError::BadRequest(
            "The built-in default module cannot be deleted".into(),
        ));
    }

    let referencing_roles = store::get_roles_referencing_module(&conn, &module.name).await?;
    if !referencing_roles.is_empty() {
        return Err(HttpError::BadRequest(format!(
            "Cannot delete module '{}': referenced by roles: {}",
            module.name,
            referencing_roles.join(", ")
        )));
    }

    store::delete_module(&conn, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Upload handlers ───────────────────────────────────────────────────────────

/// Accept a streaming OSM archive upload.
///
/// The request body is streamed to a temporary file while an upload tracking
/// record is created and returned immediately with HTTP 202 Accepted. A
/// background task continues streaming the body, updates progress, and calls
/// [`crate::osm::upload::process_upload`] once all bytes are received.
///
/// The filename for the upload record is taken from the `Content-Disposition`
/// request header's `filename=` parameter (both quoted and unquoted forms are
/// supported). Falls back to `"module.tar.zst"` when the header is absent or
/// contains no filename parameter.
async fn upload_osm(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> Result<(StatusCode, Json<store::OsmUpload>), HttpError> {
    let filename = extract_content_disposition_filename(&headers)
        .unwrap_or_else(|| "module.tar.zst".to_string());
    let conn = state.connection_factory.open().await?;
    let upload = store::create_upload(&conn, &filename, None).await?;

    let nonce: u64 = rand::random();
    let temp_path =
        std::env::temp_dir().join(format!("osm-upload-{}-{:016x}.tar.zst", upload.id, nonce));
    let upload_id = upload.id;
    let conn_factory = state.connection_factory.clone();
    let image_store = state.image_store.clone();

    tokio::spawn(async move {
        if let Err(e) = stream_upload_to_temp(
            body,
            upload_id,
            temp_path.clone(),
            conn_factory.clone(),
            image_store.clone(),
        )
        .await
        {
            log::error!("OSM upload {upload_id} failed during streaming: {e:#}");
            let _ = mark_upload_failed(upload_id, &e.to_string(), conn_factory).await;
        }
    });

    Ok((StatusCode::ACCEPTED, Json(upload)))
}

/// Extract the `filename=` parameter from a `Content-Disposition` header value.
///
/// Handles both quoted (`filename="foo.tar.zst"`) and unquoted (`filename=foo.tar.zst`)
/// forms. Returns `None` when the header is absent or contains no `filename=` parameter.
fn extract_content_disposition_filename(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::CONTENT_DISPOSITION)?
        .to_str()
        .ok()?;

    let filename_start = value.find("filename=")?;
    let after_key = &value[filename_start + "filename=".len()..];

    let filename = if let Some(inner) = after_key.strip_prefix('"') {
        // Quoted form: filename="foo.tar.zst" — find the closing quote.
        let end = inner.find('"').unwrap_or(inner.len());
        &inner[..end]
    } else {
        // Unquoted form: filename=foo.tar.zst — ends at `;`, whitespace, or end of string.
        let end = after_key
            .find(|c: char| c == ';' || c.is_whitespace())
            .unwrap_or(after_key.len());
        &after_key[..end]
    };

    if filename.is_empty() {
        None
    } else {
        Some(filename.to_string())
    }
}

/// Stream the request body to `temp_path`, update progress, then hand off to
/// the processing pipeline.
async fn stream_upload_to_temp(
    body: axum::body::Body,
    upload_id: i64,
    temp_path: std::path::PathBuf,
    conn_factory: Arc<dyn crate::database::ConnectionFactory>,
    image_store: Arc<crate::storage::ImageStore>,
) -> anyhow::Result<()> {
    let mut stream = body.into_data_stream();
    let mut file = std::fs::File::create(&temp_path)?;
    let mut received_bytes: i64 = 0;
    let mut last_progress_update: i64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow::anyhow!("Body read error: {e}"))?;
        file.write_all(&chunk)?;
        received_bytes += chunk.len() as i64;

        if received_bytes - last_progress_update >= PROGRESS_UPDATE_INTERVAL_BYTES {
            last_progress_update = received_bytes;
            let conn = conn_factory.open().await?;
            store::update_upload_progress(&conn, upload_id, received_bytes).await?;
        }
    }

    // Flush to disk before handing off.
    file.flush()?;
    drop(file);

    // Final progress update with the true total.
    let conn = conn_factory.open().await?;
    store::update_upload_progress(&conn, upload_id, received_bytes).await?;
    drop(conn);

    crate::osm::upload::process_upload(upload_id, temp_path, conn_factory, image_store).await;

    Ok(())
}

/// Mark an upload record as failed with an error message.
async fn mark_upload_failed(
    upload_id: i64,
    error: &str,
    conn_factory: Arc<dyn crate::database::ConnectionFactory>,
) -> anyhow::Result<()> {
    let conn = conn_factory.open().await?;
    store::update_upload_status(&conn, upload_id, "failed", Some(error), None).await?;
    Ok(())
}

/// List the 50 most recent OSM upload records.
async fn list_uploads(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<store::OsmUpload>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let uploads = store::list_uploads(&conn).await?;
    Ok(Json(uploads))
}

/// Get a single OSM upload record by ID.
///
/// Returns 404 if no upload with the given ID exists.
async fn get_upload(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<store::OsmUpload>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let upload = store::get_upload(&conn, id)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM upload {id} not found")))?;
    Ok(Json(upload))
}

// ── Operating system handlers ─────────────────────────────────────────────────

/// List all OSM operating system entries across every module.
async fn list_all_os(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<store::OsmOperatingSystem>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let systems = store::list_all_operating_systems(&conn).await?;
    Ok(Json(systems))
}

/// List all OSM operating system entries belonging to a specific module.
///
/// Returns 404 if no module with the given ID exists.
async fn list_module_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<store::OsmOperatingSystem>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    // Verify the module exists before listing its OS entries.
    store::get_module(&conn, id)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM module {id} not found")))?;
    let systems = store::list_operating_systems(&conn, id).await?;
    Ok(Json(systems))
}

/// Disable an OSM operating system entry.
///
/// Returns 204 on success.
async fn disable_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    store::set_os_disabled(&conn, id, true)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM operating system {id} not found")))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Enable an OSM operating system entry.
///
/// Returns 204 on success.
async fn enable_os(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    store::set_os_disabled(&conn, id, false)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM operating system {id} not found")))?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Export handler ────────────────────────────────────────────────────────────

/// Download the original OSM archive for a module.
///
/// Bundled (built-in) modules have no stored archive, so this endpoint returns
/// 404 for them. Uploaded modules stream the archive from the image store with
/// an appropriate `Content-Disposition` header.
async fn export_module(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, HttpError> {
    let conn = state.connection_factory.open().await?;
    let module = store::get_module(&conn, id)
        .await
        .map_err(|_| HttpError::NotFound(format!("OSM module {id} not found")))?;

    let archive_path = module.archive_path.ok_or_else(|| {
        HttpError::NotFound(format!(
            "Module '{}' is a built-in module and has no downloadable archive",
            module.name
        ))
    })?;

    let filename = archive_path
        .split('/')
        .next_back()
        .unwrap_or("module.tar.zst")
        .to_string();

    let (stream, _size) = state.image_store.download(&archive_path).await?;
    let body = Body::from_stream(stream);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zstd")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(body)
        .expect("building export response");

    Ok(response)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn header_map_with_content_disposition(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(
            axum::http::header::CONTENT_DISPOSITION,
            value.parse().unwrap(),
        );
        map
    }

    #[test]
    fn test_extract_quoted_filename() {
        let headers =
            header_map_with_content_disposition("attachment; filename=\"my-module.tar.zst\"");
        assert_eq!(
            extract_content_disposition_filename(&headers),
            Some("my-module.tar.zst".to_string())
        );
    }

    #[test]
    fn test_extract_unquoted_filename() {
        let headers = header_map_with_content_disposition("attachment; filename=my-module.tar.zst");
        assert_eq!(
            extract_content_disposition_filename(&headers),
            Some("my-module.tar.zst".to_string())
        );
    }

    #[test]
    fn test_extract_filename_only_disposition() {
        let headers = header_map_with_content_disposition("filename=\"archive.tar.zst\"");
        assert_eq!(
            extract_content_disposition_filename(&headers),
            Some("archive.tar.zst".to_string())
        );
    }

    #[test]
    fn test_extract_no_filename_parameter() {
        let headers = header_map_with_content_disposition("attachment");
        assert_eq!(extract_content_disposition_filename(&headers), None);
    }

    #[test]
    fn test_extract_absent_header() {
        let headers = HeaderMap::new();
        assert_eq!(extract_content_disposition_filename(&headers), None);
    }

    #[test]
    fn test_extract_empty_quoted_filename_returns_none() {
        let headers = header_map_with_content_disposition("attachment; filename=\"\"");
        assert_eq!(extract_content_disposition_filename(&headers), None);
    }

    #[test]
    fn test_extract_filename_with_trailing_semicolon_params() {
        // Unquoted form followed by another parameter.
        let headers =
            header_map_with_content_disposition("form-data; filename=upload.tar.zst; size=1234");
        assert_eq!(
            extract_content_disposition_filename(&headers),
            Some("upload.tar.zst".to_string())
        );
    }
}
