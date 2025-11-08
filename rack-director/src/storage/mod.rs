mod local;
mod s3;

pub use local::LocalImageStore;
pub use s3::S3ImageStore;

use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

/// Configuration for image storage backend
#[derive(Debug, Clone)]
pub enum ImageStoreConfig {
    Local {
        path: PathBuf,
        base_url: String, // HTTP URL base for serving files
    },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        base_url: String, // HTTP URL base for serving files (if using CDN or direct access)
    },
}

/// Trait for storing and retrieving OS images, kernels, initramfs, and install scripts
#[async_trait]
pub trait ImageStore: Send + Sync {
    /// Upload data to the store at the given path
    async fn upload(&self, path: &str, data: Vec<u8>) -> Result<()>;

    /// Download data from the store at the given path
    async fn download(&self, path: &str) -> Result<Vec<u8>>;

    /// Delete data at the given path (currently only used in tests)
    #[allow(dead_code)]
    async fn delete(&self, path: &str) -> Result<()>;

    /// Check if a file exists at the given path (currently only used in tests)
    #[allow(dead_code)]
    async fn exists(&self, path: &str) -> Result<bool>;

    /// List all files with the given prefix (currently only used in tests)
    #[allow(dead_code)]
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Get the HTTP URL for a file (for iPXE to download)
    fn get_url(&self, path: &str) -> String;
}

/// Create an ImageStore from configuration
pub async fn create_image_store(config: ImageStoreConfig) -> Result<Arc<dyn ImageStore>> {
    match config {
        ImageStoreConfig::Local { path, base_url } => {
            Ok(Arc::new(LocalImageStore::new(path, base_url)?))
        }
        ImageStoreConfig::S3 {
            endpoint,
            bucket,
            region,
            access_key,
            secret_key,
            base_url,
        } => Ok(Arc::new(
            S3ImageStore::new(endpoint, bucket, region, access_key, secret_key, base_url).await?,
        )),
    }
}
