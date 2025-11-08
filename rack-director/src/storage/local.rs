use super::ImageStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Local filesystem-based image storage
pub struct LocalImageStore {
    root_path: PathBuf,
    base_url: String,
}

impl LocalImageStore {
    pub fn new(root_path: PathBuf, base_url: String) -> Result<Self> {
        // Create root directory if it doesn't exist
        std::fs::create_dir_all(&root_path).context("Failed to create image storage directory")?;

        Ok(Self {
            root_path,
            base_url,
        })
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        self.root_path.join(path)
    }

    /// Ensure parent directories exist for a file path
    async fn ensure_parent_dirs(&self, file_path: &Path) -> Result<()> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }
        Ok(())
    }
}

#[async_trait]
impl ImageStore for LocalImageStore {
    async fn upload(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let file_path = self.resolve_path(path);
        self.ensure_parent_dirs(&file_path).await?;

        let mut file = fs::File::create(&file_path)
            .await
            .context("Failed to create file")?;

        file.write_all(&data)
            .await
            .context("Failed to write file")?;

        file.sync_all().await.context("Failed to sync file")?;

        log::info!("Uploaded file to local storage: {}", file_path.display());
        Ok(())
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>> {
        let file_path = self.resolve_path(path);

        let data = fs::read(&file_path)
            .await
            .context(format!("Failed to read file: {}", file_path.display()))?;

        log::debug!(
            "Downloaded file from local storage: {}",
            file_path.display()
        );
        Ok(data)
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let file_path = self.resolve_path(path);

        fs::remove_file(&file_path)
            .await
            .context("Failed to delete file")?;

        log::info!("Deleted file from local storage: {}", file_path.display());
        Ok(())
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let file_path = self.resolve_path(path);
        Ok(file_path.exists())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let prefix_path = self.resolve_path(prefix);
        let mut results = Vec::new();

        // If prefix is a directory, list all files recursively
        if prefix_path.is_dir() {
            let mut stack = vec![prefix_path.clone()];

            while let Some(dir) = stack.pop() {
                let mut entries = fs::read_dir(&dir)
                    .await
                    .context("Failed to read directory")?;

                while let Some(entry) = entries
                    .next_entry()
                    .await
                    .context("Failed to read directory entry")?
                {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        // Convert absolute path back to relative path
                        if let Ok(rel_path) = path.strip_prefix(&self.root_path) {
                            results.push(rel_path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn get_url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_upload_download() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalImageStore::new(
            temp_dir.path().to_path_buf(),
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        let test_data = b"Hello, World!".to_vec();
        store.upload("test.txt", test_data.clone()).await.unwrap();

        let downloaded = store.download("test.txt").await.unwrap();
        assert_eq!(downloaded, test_data);
    }

    #[tokio::test]
    async fn test_exists() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalImageStore::new(
            temp_dir.path().to_path_buf(),
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        assert!(!store.exists("nonexistent.txt").await.unwrap());

        store.upload("exists.txt", b"data".to_vec()).await.unwrap();
        assert!(store.exists("exists.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalImageStore::new(
            temp_dir.path().to_path_buf(),
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        store.upload("delete.txt", b"data".to_vec()).await.unwrap();
        assert!(store.exists("delete.txt").await.unwrap());

        store.delete("delete.txt").await.unwrap();
        assert!(!store.exists("delete.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_nested_paths() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalImageStore::new(
            temp_dir.path().to_path_buf(),
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        let test_data = b"nested data".to_vec();
        store
            .upload("os/1/kernel/vmlinuz", test_data.clone())
            .await
            .unwrap();

        let downloaded = store.download("os/1/kernel/vmlinuz").await.unwrap();
        assert_eq!(downloaded, test_data);
    }

    #[tokio::test]
    async fn test_get_url() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalImageStore::new(
            temp_dir.path().to_path_buf(),
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        let url = store.get_url("os/1/kernel/vmlinuz");
        assert_eq!(url, "http://localhost:8080/images/os/1/kernel/vmlinuz");
    }
}
