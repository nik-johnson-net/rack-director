use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
};

use crate::http::AppState;
use crate::http::error::Error;
use crate::osm::store;

/// Path parameters for the OSM file endpoint.
/// URL: /cnc/osm/{module}/{version}/{os_dir}/{*file}
///
/// The `file` wildcard may include subdirectories, e.g. `x86-64/vmlinuz`.
#[derive(Debug, serde::Deserialize)]
pub struct OsmFilePath {
    pub module: String,
    pub version: String,
    pub os_dir: String,
    pub file: String,
}

/// Serve a static file from an OSM module.
///
/// For bundled modules, files are served directly from the bundled OSM directory
/// on disk (set via `--bundled-osm-path`).  For uploaded modules, files are served
/// from the image store.
///
/// The URL directly encodes the storage path, so no database lookup is needed for
/// uploaded modules.  Bundled modules require a DB lookup to confirm the source.
pub async fn osm_file_handler(
    State(state): State<Arc<AppState>>,
    Path(params): Path<OsmFilePath>,
) -> Result<Response<Body>, Error> {
    validate_path_components(&params)?;

    // Check whether this module is bundled so we can serve from disk.
    if let Some(bundled_path) = &state.bundled_osm_path
        && let Ok(conn) = state.connection_factory.open().await
        && let Ok(module) = store::get_module_by_name(&conn, &params.module).await
        && module.source == "bundled"
    {
        return serve_bundled_file(bundled_path, &params).await;
    }

    serve_from_image_store(&state, &params).await
}

/// Validate all path components to prevent directory traversal.
fn validate_path_components(params: &OsmFilePath) -> Result<(), Error> {
    for component in [&params.module, &params.version, &params.os_dir] {
        if component.contains("..") || component.contains('/') || component.contains('\\') {
            return Err(Error::BadRequest("Invalid path component".to_string()));
        }
    }
    if params.file.contains("..") {
        return Err(Error::BadRequest("Invalid file path".to_string()));
    }
    Ok(())
}

/// Serve a file from the on-disk bundled OSM directory.
///
/// Path structure: `{bundled_osm_path}/{os_dir}/{file}`
/// The module name and version are not part of the on-disk layout — the
/// bundled path already points to the versioned module directory.
async fn serve_bundled_file(
    bundled_osm_path: &std::path::Path,
    params: &OsmFilePath,
) -> Result<Response<Body>, Error> {
    let disk_path = bundled_osm_path.join(&params.os_dir).join(&params.file);

    let data = tokio::fs::read(&disk_path).await.map_err(|e| {
        log::debug!(
            "Bundled OSM file not found: {} ({})",
            disk_path.display(),
            e
        );
        Error::NotFound(format!(
            "File not found: {}/{}/{}",
            params.os_dir, params.file, e
        ))
    })?;

    let size = data.len() as u64;
    let content_type = determine_content_type(&params.file);
    let body = Body::from(data);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, size)
        .body(body)
        .expect("response build should not fail"))
}

/// Serve a file from the image store (for uploaded OSM modules).
async fn serve_from_image_store(
    state: &AppState,
    params: &OsmFilePath,
) -> Result<Response<Body>, Error> {
    let storage_path = format!(
        "osm/{}/{}/{}/{}",
        params.module, params.version, params.os_dir, params.file
    );

    let (stream, size) = state
        .image_store
        .download(&storage_path)
        .await
        .map_err(|e| {
            log::debug!("OSM file not found: {} ({})", storage_path, e);
            Error::NotFound(format!("File not found: {}", storage_path))
        })?;

    let content_type = determine_content_type(&params.file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, size)
        .body(body)
        .expect("response build should not fail"))
}

/// Determine the HTTP content type from a file's extension.
fn determine_content_type(file: &str) -> &'static str {
    let ext = file.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match ext {
        "hbs" => "text/plain",
        "ko" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_path_components_accepted() {
        let params = OsmFilePath {
            module: "Default".to_string(),
            version: "1.0.0".to_string(),
            os_dir: "ubuntu".to_string(),
            file: "x86-64/vmlinuz".to_string(),
        };
        assert!(validate_path_components(&params).is_ok());
    }

    #[test]
    fn path_traversal_in_module_rejected() {
        let params = OsmFilePath {
            module: "../etc".to_string(),
            version: "1.0.0".to_string(),
            os_dir: "ubuntu".to_string(),
            file: "vmlinuz".to_string(),
        };
        assert!(validate_path_components(&params).is_err());
    }

    #[test]
    fn path_traversal_in_file_rejected() {
        let params = OsmFilePath {
            module: "Default".to_string(),
            version: "1.0.0".to_string(),
            os_dir: "ubuntu".to_string(),
            file: "../etc/passwd".to_string(),
        };
        assert!(validate_path_components(&params).is_err());
    }

    #[test]
    fn slash_in_os_dir_rejected() {
        let params = OsmFilePath {
            module: "Default".to_string(),
            version: "1.0.0".to_string(),
            os_dir: "ubuntu/hack".to_string(),
            file: "vmlinuz".to_string(),
        };
        assert!(validate_path_components(&params).is_err());
    }

    #[test]
    fn content_type_for_known_extensions() {
        assert_eq!(determine_content_type("install.yaml.hbs"), "text/plain");
        assert_eq!(
            determine_content_type("module.ko"),
            "application/octet-stream"
        );
        assert_eq!(
            determine_content_type("vmlinuz"),
            "application/octet-stream"
        );
        assert_eq!(
            determine_content_type("x86-64/vmlinuz"),
            "application/octet-stream"
        );
    }
}
