use std::sync::Arc;

use axum::{
    body::Body,
    extract::{self, State},
    http::Response,
    http::header,
    response::IntoResponse,
};
use log::warn;
use tokio::io::AsyncReadExt;

use crate::http::{AppState, error::Error};

/// HTTP endpoint to serve boot files (ipxe.efi, undionly.kpxe) for UEFI HTTP Boot
///
/// This endpoint serves firmware boot files over HTTP for modern UEFI clients that support
/// HTTP Boot (architectures 14/15/16). Files are served from the BootFileProvider which
/// enforces path validation security.
///
/// # Route
/// GET /cnc/boot/{filename}
///
/// # Security
/// - Path traversal protection via canonicalization
/// - Files must be within the configured boot files directory
///
/// # Arguments
///
/// * `state` - The application state containing the boot file provider
/// * `filename` - The requested boot file name from the URL path
///
/// # Returns
///
/// Returns HTTP 200 with the file contents and `application/octet-stream` content type,
/// or HTTP 404 if the file doesn't exist or fails validation.
///
/// # Errors
///
/// Returns `Error::NotFound` if:
/// - The file path is invalid or attempts directory traversal
/// - The file does not exist on the filesystem
/// - There is an I/O error reading the file
pub async fn boot_file_handler(
    State(state): State<Arc<AppState>>,
    extract::Path(filename): extract::Path<String>,
) -> Result<impl IntoResponse, Error> {
    // Get a reader for the file (validates path)
    let mut reader = state
        .boot_file_provider
        .get_file(&filename)
        .await
        .map_err(|e| {
            warn!("Boot file request denied for '{}': {}", filename, e);
            Error::NotFound(format!("Boot file not found: {}", filename))
        })?;

    // Read the entire file into memory
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.map_err(|e| {
        warn!("Failed to read boot file '{}': {}", filename, e);
        Error::NotFound(format!("Failed to read boot file: {}", filename))
    })?;

    let body = Body::from(bytes);
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use std::io::Write;
    use tempfile::TempDir;
    use tower::util::ServiceExt;

    use crate::{
        boot_files::FilesystemBootFileProvider,
        database,
        director::Director,
        storage::{ImageStore, ImageStoreConfig},
    };

    /// Create a test AppState with temporary directory and sample boot files
    async fn create_test_state() -> (Arc<AppState>, TempDir) {
        let temp_dir = TempDir::new().unwrap();

        // Create database
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let db_tokio = Arc::new(tokio::sync::Mutex::new(db));

        // Create boot files directory with test files
        let boot_files_dir = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_dir).unwrap();

        let ipxe_path = boot_files_dir.join("ipxe.efi");
        let kpxe_path = boot_files_dir.join("undionly.kpxe");
        let unauthorized_path = boot_files_dir.join("unauthorized.bin");

        std::fs::File::create(&ipxe_path)
            .unwrap()
            .write_all(b"IPXE_EFI_BINARY_DATA")
            .unwrap();

        std::fs::File::create(&kpxe_path)
            .unwrap()
            .write_all(b"KPXE_BINARY_DATA")
            .unwrap();

        std::fs::File::create(&unauthorized_path)
            .unwrap()
            .write_all(b"UNAUTHORIZED_DATA")
            .unwrap();

        // Create boot file provider
        let boot_file_provider =
            Arc::new(FilesystemBootFileProvider::new(boot_files_dir.clone()).unwrap());

        // Create agent-images directory
        let agent_images_path = temp_dir.path().join("agent-images");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        // Create storage path for image store
        let storage_path = temp_dir.path().join("images");

        let image_store = ImageStore::new(ImageStoreConfig::Local {
            path: storage_path,
            base_url: "http://localhost:8080".into(),
        })
        .unwrap();

        let state = Arc::new(AppState {
            director: Director::new(db_tokio.clone()),
            dhcp_store: crate::dhcp::DhcpStore::new(db_tokio.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db_tokio.clone()),
            roles_store: crate::roles::RolesStore::new(db_tokio),
            agent_images_path,
            boot_file_provider,
        });

        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_boot_file_handler_success_ipxe_efi() {
        let (state, _temp_dir) = create_test_state().await;

        let app = Router::new()
            .route(
                "/cnc/boot/{filename}",
                axum::routing::get(boot_file_handler),
            )
            .with_state(state);

        let request = Request::builder()
            .uri("/cnc/boot/ipxe.efi")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"IPXE_EFI_BINARY_DATA");
    }

    #[tokio::test]
    async fn test_boot_file_handler_nonexistent_file() {
        let (state, _temp_dir) = create_test_state().await;

        let app = Router::new()
            .route(
                "/cnc/boot/{filename}",
                axum::routing::get(boot_file_handler),
            )
            .with_state(state);

        let request = Request::builder()
            .uri("/cnc/boot/nonexistent.efi")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_boot_file_handler_path_traversal_blocked() {
        let (state, temp_dir) = create_test_state().await;

        // Create a file outside the boot directory
        let parent_dir = temp_dir.path();
        let outside_file = parent_dir.join("secret.txt");
        std::fs::File::create(&outside_file)
            .unwrap()
            .write_all(b"SECRET")
            .unwrap();

        let app = Router::new()
            .route(
                "/cnc/boot/{filename}",
                axum::routing::get(boot_file_handler),
            )
            .with_state(state);

        // Attempt path traversal to access file outside boot directory
        let request = Request::builder()
            .uri("/cnc/boot/../secret.txt")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should be blocked by canonicalization - returns 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Clean up
        std::fs::remove_file(&outside_file).ok();
    }

    #[tokio::test]
    async fn test_boot_file_handler_content_type() {
        let (state, _temp_dir) = create_test_state().await;

        let app = Router::new()
            .route(
                "/cnc/boot/{filename}",
                axum::routing::get(boot_file_handler),
            )
            .with_state(state);

        let request = Request::builder()
            .uri("/cnc/boot/ipxe.efi")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify Content-Type header
        let content_type = response.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(content_type, "application/octet-stream");
    }
}
