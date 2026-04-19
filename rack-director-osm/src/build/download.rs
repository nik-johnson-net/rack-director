use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Download a file from `url` and save it to `dest`.
pub async fn download_file(url: &str, dest: &Path) -> Result<()> {
    log::info!("Downloading {} -> {}", url, dest.display());

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("failed to GET {url}"))?;

    if !response.status().is_success() {
        bail!("HTTP {} for {url}", response.status());
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read response body from {url}"))?;

    fs::write(dest, &bytes).with_context(|| format!("failed to write to {}", dest.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration-ish test using a local HTTP server (mockito)
    #[tokio::test]
    async fn test_download_file_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/vmlinuz")
            .with_status(200)
            .with_body(b"kernel-content")
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("vmlinuz");

        download_file(&format!("{}/vmlinuz", server.url()), &dest)
            .await
            .unwrap();

        mock.assert_async().await;
        assert_eq!(fs::read(&dest).unwrap(), b"kernel-content");
    }

    #[tokio::test]
    async fn test_download_file_404_returns_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("missing");

        let result = download_file(&format!("{}/missing", server.url()), &dest).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_download_file_creates_parent_dirs() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/file")
            .with_status(200)
            .with_body(b"data")
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("subdir").join("file");

        download_file(&format!("{}/file", server.url()), &dest)
            .await
            .unwrap();
        assert!(dest.exists());
    }
}
