use object_store::aws::AmazonS3Builder;
use object_store::buffered::BufWriter;
use object_store::{ObjectStore, ObjectStoreExt};

use anyhow::Result;
use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt};
use object_store::local::LocalFileSystem;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

/// Type alias for data streams used in upload/download operations
pub type DataStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Configuration for image storage backend
#[derive(Debug, Clone)]
pub enum ImageStoreConfig {
    #[cfg(test)]
    Memory {},
    Local {
        path: PathBuf,
    },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
    },
}

// /// Trait for storing and retrieving OS images, kernels, initramfs, and install scripts
// #[async_trait]
// pub trait ImageStore: Send + Sync {
//     /// Upload data from a stream to the store at the given path
//     async fn upload(&self, path: &str, stream: DataStream) -> Result<()>;

//     /// Download data from the store as a stream
//     async fn download(&self, path: &str) -> Result<DataStream>;

//     /// Delete data at the given path (currently only used in tests)
//     #[allow(dead_code)]
//     async fn delete(&self, path: &str) -> Result<()>;

//     /// Check if a file exists at the given path (currently only used in tests)
//     #[allow(dead_code)]
//     async fn exists(&self, path: &str) -> Result<bool>;

//     /// List all files with the given prefix (currently only used in tests)
//     #[allow(dead_code)]
//     async fn list(&self, prefix: &str) -> Result<Vec<String>>;
// }

pub struct ImageStore {
    kind: String,
    location: String,
    client: Arc<Box<dyn ObjectStore>>,
}

impl ImageStore {
    pub fn new(config: ImageStoreConfig) -> Result<Self> {
        let store = match config {
            #[cfg(test)]
            ImageStoreConfig::Memory {} => {
                use object_store::memory::InMemory;

                let client = InMemory::new();
                ImageStore {
                    kind: "memory".to_owned(),
                    location: "".to_owned(),
                    client: Arc::new(Box::new(client)),
                }
            }
            ImageStoreConfig::Local { path } => {
                let client = LocalFileSystem::new_with_prefix(&path)?.with_automatic_cleanup(true);
                ImageStore {
                    kind: "local".to_owned(),
                    location: path.to_string_lossy().to_string(),
                    client: Arc::new(Box::new(client)),
                }
            }
            ImageStoreConfig::S3 {
                endpoint,
                bucket,
                region,
                access_key,
                secret_key,
            } => {
                let client = AmazonS3Builder::from_env()
                    .with_access_key_id(access_key)
                    .with_secret_access_key(secret_key)
                    .with_bucket_name(&bucket)
                    .with_endpoint(&endpoint)
                    .with_region(region)
                    .build()?;

                ImageStore {
                    kind: "S3".to_owned(),
                    location: format!("{}/{}", endpoint, bucket),
                    client: Arc::new(Box::new(client)),
                }
            }
        };

        log::info!(
            "Initialized {} image store at {}",
            store.kind,
            store.location
        );

        Ok(store)
    }

    /// Test Convenience method for creating an in-memory image store.
    #[cfg(test)]
    pub fn memory() -> Self {
        Self::new(ImageStoreConfig::Memory {}).unwrap()
    }

    /// Upload data from a stream to the store at the given path
    pub async fn upload(&self, path: &str, mut stream: DataStream) -> Result<()> {
        let mut writer = BufWriter::new(self.client.clone(), path.into());

        while let Some(result) = stream.next().await {
            match result {
                Ok(data) => {
                    writer.put(data).await?;
                }
                Err(e) => {
                    log::debug!("Error reading next chunk. Did sender go away? {:?}", e);
                    writer.abort().await?;
                    anyhow::bail!(e);
                }
            }
        }

        writer.shutdown().await?;
        log::debug!("Uploaded object {}/{}", self.location, path);
        Ok(())
    }

    /// Download data from the store as a stream, returning the stream and file size in bytes.
    ///
    /// The file size is extracted from the object metadata before consuming the result into a
    /// stream, enabling callers to set a `Content-Length` header on HTTP responses so that
    /// clients (e.g. iPXE) do not fall back to chunked transfer encoding.
    pub async fn download(&self, path: &str) -> Result<(DataStream, u64)> {
        let result = self.client.get(&path.into()).await?;
        let size = result.meta.size as u64;
        let datastream = result.into_stream().map_err(|e| e.into());
        Ok((Box::pin(datastream), size))
    }

    /// Delete data at the given path (currently only used in tests)
    #[allow(dead_code)]
    pub async fn delete(&self, path: &str) -> Result<()> {
        self.client.delete(&path.into()).await?;

        log::debug!("Deleted object {}/{}", self.location, path);
        Ok(())
    }

    /// Check if a file exists at the given path (currently only used in tests)
    #[allow(dead_code)]
    pub async fn exists(&self, path: &str) -> Result<bool> {
        match self.client.head(&path.into()).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { path: _, source: _ }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// List all files with the given prefix (currently only used in tests)
    #[allow(dead_code)]
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let list_stream = self.client.list(Some(&prefix.into()));

        list_stream
            .map_ok(|object| object.location.to_string())
            .map_err(|e| e.into())
            .try_collect()
            .await
    }
}
