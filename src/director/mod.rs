use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;

use crate::director::store::DirectorStore;
use crate::tftp::Handler;
use crate::tftp::Reader;

mod store;

pub enum BootTarget {
    LocalDisk,
    NetBoot {
        ramdisk: String,
        kernel: String,
        cmdline: String,
    },
}

#[derive(Clone)]
pub struct Director {
    store: DirectorStore,
}

impl Director {
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>) -> Self {
        let store = DirectorStore::new(conn);
        Director { store }
    }

    pub async fn register_device(&self, uuid: &str) -> anyhow::Result<()> {
        self.store.register_device(uuid).await?;

        Ok(())
    }

    pub async fn next_boot_target(&self, uuid: &str) -> anyhow::Result<BootTarget> {
        self.store
            .update_device_last_seen(uuid)
            .await
            .expect("update device last seen should not fail");

        Ok(BootTarget::LocalDisk)
    }
}

pub struct DirectorTftpHandler {
    root: PathBuf,
}

impl DirectorTftpHandler {
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        DirectorTftpHandler { root: root.into() }
    }
}

impl Handler for DirectorTftpHandler {
    type Reader = DirectorTftpReader;

    async fn create_reader(&self, filename: &str) -> anyhow::Result<Self::Reader> {
        match filename {
            "ipxe.efi" | "undionly.kpxe" => {
                let reader = DirectorTftpReader::open(&self.root.join(filename)).await?;
                Ok(reader)
            }
            _ => Err(anyhow::anyhow!("Unsupported file: {}", filename)),
        }
    }
}

pub struct DirectorTftpReader {
    file: BufReader<tokio::fs::File>,
}

impl DirectorTftpReader {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let file = tokio::fs::File::open(path).await?;
        Ok(DirectorTftpReader {
            file: BufReader::new(file),
        })
    }
}

impl Reader for DirectorTftpReader {
    async fn read(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut chunk = vec![0; 512]; // Read in chunks of 512 bytes
        let _ = self.file.read(&mut chunk).await?;
        Ok(chunk)
    }
}
