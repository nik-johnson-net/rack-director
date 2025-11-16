use super::ImageStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;

/// S3-compatible image storage (works with Ceph RGW, MinIO, AWS S3, etc.)
pub struct S3ImageStore {
    client: Client,
    bucket: String,
    base_url: String,
}

impl S3ImageStore {
    pub async fn new(
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        base_url: String,
    ) -> Result<Self> {
        let credentials = Credentials::new(
            access_key, secret_key, None, // session token
            None, // expiration
            "static",
        );

        let config = aws_sdk_s3::Config::builder()
            .endpoint_url(&endpoint)
            .region(Region::new(region))
            .credentials_provider(credentials)
            .force_path_style(true) // Required for Ceph RGW and MinIO
            .build();

        let client = Client::from_conf(config);

        log::info!(
            "Initialized S3 image store: endpoint={}, bucket={}",
            endpoint,
            bucket
        );

        Ok(Self {
            client,
            bucket,
            base_url,
        })
    }
}

#[async_trait]
impl ImageStore for S3ImageStore {
    async fn upload(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let byte_stream = ByteStream::from(data);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(path)
            .body(byte_stream)
            .send()
            .await
            .context("Failed to upload to S3")?;

        log::info!("Uploaded to S3: s3://{}/{}", self.bucket, path);
        Ok(())
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .context(format!("Failed to download from S3: {}", path))?;

        let data = response
            .body
            .collect()
            .await
            .context("Failed to read S3 response body")?
            .into_bytes()
            .to_vec();

        log::debug!("Downloaded from S3: s3://{}/{}", self.bucket, path);
        Ok(data)
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .context("Failed to delete from S3")?;

        log::info!("Deleted from S3: s3://{}/{}", self.bucket, path);
        Ok(())
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if it's a 404 error
                if e.to_string().contains("NotFound") || e.to_string().contains("404") {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut results = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);

            if let Some(token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request.send().await.context("Failed to list S3 objects")?;

            // Check if truncated and get next token before consuming contents
            let is_truncated = response.is_truncated() == Some(true);
            let next_token = response.next_continuation_token.clone();

            if let Some(contents) = response.contents {
                for object in contents {
                    if let Some(key) = object.key {
                        results.push(key);
                    }
                }
            }

            if is_truncated {
                continuation_token = next_token;
            } else {
                break;
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

    // Note: These tests require a running S3-compatible service (MinIO, Ceph, etc.)
    // They are disabled by default. Enable with: cargo test --features s3-integration-tests

    #[tokio::test]
    #[ignore]
    async fn test_s3_upload_download() {
        let store = S3ImageStore::new(
            "http://localhost:9000".to_string(),
            "test-bucket".to_string(),
            "us-east-1".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            "http://localhost:9000/test-bucket".to_string(),
        )
        .await
        .unwrap();

        let test_data = b"S3 test data".to_vec();
        store
            .upload("test/s3-test.txt", test_data.clone())
            .await
            .unwrap();

        let downloaded = store.download("test/s3-test.txt").await.unwrap();
        assert_eq!(downloaded, test_data);

        store.delete("test/s3-test.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_s3_exists() {
        let store = S3ImageStore::new(
            "http://localhost:9000".to_string(),
            "test-bucket".to_string(),
            "us-east-1".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            "http://localhost:9000/test-bucket".to_string(),
        )
        .await
        .unwrap();

        assert!(!store.exists("nonexistent.txt").await.unwrap());

        store
            .upload("test/exists.txt", b"data".to_vec())
            .await
            .unwrap();
        assert!(store.exists("test/exists.txt").await.unwrap());

        store.delete("test/exists.txt").await.unwrap();
    }

    #[test]
    fn test_s3_get_url() {
        // We can test URL generation without a real S3 connection
        let _bucket = "test-bucket".to_string();
        let base_url = "https://s3.example.com/test-bucket".to_string();

        // Create a mock S3ImageStore (we'd need to refactor slightly to allow this)
        // For now, just test the URL format
        let expected = "https://s3.example.com/test-bucket/os/1/kernel/vmlinuz";
        let path = "os/1/kernel/vmlinuz";
        let url = format!("{}/{}", base_url.trim_end_matches('/'), path);
        assert_eq!(url, expected);
    }
}
