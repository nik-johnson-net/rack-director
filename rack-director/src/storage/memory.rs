use std::collections::HashMap;

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::storage::ImageStore;

pub struct MemoryImageStore {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryImageStore {
    pub fn new() -> MemoryImageStore {
        MemoryImageStore {
            store: Mutex::default(),
        }
    }
}

#[async_trait]
impl ImageStore for MemoryImageStore {
    async fn upload(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let mut store = self.store.lock().await;
        store.insert(path.to_string(), data);
        Ok(())
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>> {
        let store = self.store.lock().await;
        match store.get(path) {
            Some(data) => Ok(data.clone()),
            None => Err(anyhow!("not found")),
        }
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let mut store = self.store.lock().await;
        store.remove(path);
        Ok(())
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let store = self.store.lock().await;
        Ok(store.contains_key(path))
    }

    async fn list(&self, _prefix: &str) -> Result<Vec<String>> {
        unimplemented!()
    }

    fn get_url(&self, path: &str) -> String {
        format!("http://localhost:0/images/{}", path)
    }
}
