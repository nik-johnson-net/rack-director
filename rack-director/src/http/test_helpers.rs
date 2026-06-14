//! Shared test helpers for HTTP handler tests.
//!
//! This module provides common setup utilities used across multiple HTTP API
//! test modules.  It is only compiled in test builds.

use std::sync::Arc;

use crate::{database, http::AppState};

/// Build a minimal `AppState` suitable for HTTP handler tests.
///
/// Uses an in-memory image store and a stub `FilesystemBootFileProvider` so
/// that tests can focus on the API logic rather than filesystem setup.
///
/// The `TempDir` holding agent-image and boot-file paths is intentionally
/// leaked (via `std::mem::forget`) so that the paths remain valid for the
/// full test duration.
pub fn build_test_state(conn_factory: Arc<dyn database::ConnectionFactory>) -> Arc<AppState> {
    let temp_dir = tempfile::tempdir().unwrap();
    let agent_images_path = temp_dir.path().join("agent-image");
    std::fs::create_dir_all(&agent_images_path).unwrap();
    let boot_files_path = temp_dir.path().join("boot");
    std::fs::create_dir_all(&boot_files_path).unwrap();
    let boot_file_provider =
        Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());
    let image_store =
        crate::storage::ImageStore::new(crate::storage::ImageStoreConfig::Memory {}).unwrap();
    // Leak the TempDir so the paths remain valid for the test duration.
    std::mem::forget(temp_dir);
    Arc::new(AppState {
        connection_factory: conn_factory,
        image_store: image_store.into(),
        agent_images_path,
        boot_file_provider,
        dhcp: crate::dhcp::DhcpControl::noop(),
        unprovisioned_sleep_secs: 0,
        bundled_osm_path: None,
        power_config: crate::director::power::PowerConfig::default(),
    })
}
