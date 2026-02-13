mod filesystem;

pub use filesystem::FilesystemBootFileProvider;

use anyhow::Result;
use async_trait::async_trait;
use tokio::io::BufReader;

/// Trait for providing access to boot files (iPXE binaries) for TFTP and HTTP servers.
///
/// This trait abstracts the storage and retrieval of boot files, allowing different
/// implementations for testing, production, and various storage backends.
///
/// # Security
///
/// Implementations must enforce path validation to prevent unauthorized access
/// to arbitrary files on the filesystem. Path traversal attacks should be prevented
/// using canonicalization and path prefix checks.
#[async_trait]
pub trait BootFileProvider: Send + Sync {
    /// Get a buffered reader for a boot file.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the boot file to retrieve (e.g., "snponly.efi")
    ///
    /// # Returns
    ///
    /// Returns a buffered reader for streaming the file contents.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file path is invalid or attempts directory traversal
    /// - The file does not exist
    /// - There is an I/O error opening the file
    async fn get_file(&self, filename: &str) -> Result<BufReader<tokio::fs::File>>;

    /// Get the size of a file in bytes.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the boot file
    ///
    /// # Returns
    ///
    /// Returns the file size in bytes if the file exists and passes validation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file path is invalid or attempts directory traversal
    /// - The file does not exist
    /// - There is an I/O error accessing the file metadata
    async fn filesize(&self, filename: &str) -> Result<u64>;
}
