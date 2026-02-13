use super::BootFileProvider;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::BufReader;

use crate::tftp::{Handler, TftpReader};

/// Filesystem-based boot file provider with path canonicalization security.
///
/// This provider serves boot files (such as iPXE binaries) from a local filesystem
/// directory, enforcing path validation to prevent unauthorized file access.
///
/// # Security
///
/// Path validation is performed using canonicalization to prevent directory traversal
/// attacks. The canonicalized requested path must be within the canonicalized base path.
/// Any attempt to access files outside the base directory will be rejected.
///
/// # Example
///
/// ```no_run
/// use rack_director::boot_files::{FilesystemBootFileProvider, BootFileProvider};
/// use std::path::PathBuf;
///
/// # async fn example() -> anyhow::Result<()> {
/// let provider = FilesystemBootFileProvider::new(
///     PathBuf::from("/var/lib/tftpboot"),
/// )?;
///
/// // This will succeed if the file exists
/// let reader = provider.get_file("snponly.efi").await?;
///
/// // This will fail due to path traversal attempt
/// assert!(provider.get_file("../etc/passwd").await.is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct FilesystemBootFileProvider {
    base_path: PathBuf,
    canonical_base_path: PathBuf,
}

impl FilesystemBootFileProvider {
    /// Create a new filesystem boot file provider.
    ///
    /// # Arguments
    ///
    /// * `base_path` - The root directory containing boot files
    ///
    /// # Returns
    ///
    /// Returns a new provider instance if the base path exists and can be canonicalized.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The base path does not exist or is not a directory
    /// - The base path cannot be canonicalized
    pub fn new(base_path: PathBuf) -> Result<Self> {
        // Verify base path exists and is a directory
        if !base_path.exists() {
            anyhow::bail!(
                "Boot files directory does not exist: {}",
                base_path.display()
            );
        }

        if !base_path.is_dir() {
            anyhow::bail!(
                "Boot files path is not a directory: {}",
                base_path.display()
            );
        }

        // Canonicalize the base path for security validation
        let canonical_base_path = base_path
            .canonicalize()
            .context("Failed to canonicalize boot files directory")?;

        Ok(Self {
            base_path,
            canonical_base_path,
        })
    }

    /// Validate and resolve a filename to a full filesystem path.
    ///
    /// This is a security-critical function that prevents directory traversal attacks
    /// by ensuring the resolved path is within the base directory.
    ///
    /// # Security
    ///
    /// The validation process:
    /// 1. Join the filename to the base path
    /// 2. Canonicalize the result
    /// 3. Check if the canonicalized path starts with the canonicalized base path
    ///
    /// # Arguments
    ///
    /// * `filename` - The requested filename
    ///
    /// # Returns
    ///
    /// Returns the validated canonical path if the file is within the base directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path cannot be canonicalized (file doesn't exist)
    /// - The resolved path is outside the base directory (directory traversal attempt)
    fn validate_and_resolve_path(&self, filename: &str) -> Result<PathBuf> {
        let requested_path = self.base_path.join(filename);

        // Canonicalize the requested path
        let canonical_path = requested_path.canonicalize().context(format!(
            "Failed to access boot file: {}",
            requested_path.display()
        ))?;

        // Security check: ensure the canonical path is within the base directory
        if !canonical_path.starts_with(&self.canonical_base_path) {
            anyhow::bail!(
                "Access denied: path '{}' is outside boot files directory",
                filename
            );
        }

        Ok(canonical_path)
    }
}

#[async_trait]
impl BootFileProvider for FilesystemBootFileProvider {
    async fn get_file(&self, filename: &str) -> Result<BufReader<tokio::fs::File>> {
        // Security: Validate path and resolve to canonical path
        let file_path = self.validate_and_resolve_path(filename)?;

        let file = fs::File::open(&file_path)
            .await
            .context(format!("Failed to open boot file: {}", file_path.display()))?;

        log::debug!("Opened boot file: {}", filename);
        Ok(BufReader::new(file))
    }

    async fn filesize(&self, filename: &str) -> Result<u64> {
        // Security: Validate path and resolve to canonical path
        let file_path = self.validate_and_resolve_path(filename)?;

        let metadata = fs::metadata(&file_path).await.context(format!(
            "Failed to get file metadata: {}",
            file_path.display()
        ))?;

        Ok(metadata.len())
    }
}

impl Handler for FilesystemBootFileProvider {
    type Reader = TftpReader;

    async fn create_reader(&self, filename: &str, block_size: u64) -> Result<Self::Reader> {
        // Security: Validate path and resolve to canonical path
        let file_path = self.validate_and_resolve_path(filename)?;

        let reader = TftpReader::open(&file_path, block_size).await?;
        Ok(reader)
    }

    async fn filesize(&self, filename: &str) -> Result<u64> {
        // Delegate to BootFileProvider implementation
        BootFileProvider::filesize(self, filename).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    /// Create a test provider with a temporary directory and sample files.
    fn create_test_provider() -> (FilesystemBootFileProvider, TempDir) {
        let temp_dir = TempDir::new().unwrap();

        // Create test files
        let ipxe_path = temp_dir.path().join("snponly.efi");
        let kpxe_path = temp_dir.path().join("undionly.kpxe");
        let unauthorized_path = temp_dir.path().join("unauthorized.bin");

        std::fs::File::create(&ipxe_path)
            .unwrap()
            .write_all(b"IPXE_EFI_BINARY_DATA")
            .unwrap();

        std::fs::File::create(&kpxe_path)
            .unwrap()
            .write_all(b"KPXE_BINARY")
            .unwrap();

        std::fs::File::create(&unauthorized_path)
            .unwrap()
            .write_all(b"UNAUTHORIZED")
            .unwrap();

        let provider = FilesystemBootFileProvider::new(temp_dir.path().to_path_buf()).unwrap();

        (provider, temp_dir)
    }

    #[test]
    fn test_new_with_valid_directory() {
        let temp_dir = TempDir::new().unwrap();

        let result = FilesystemBootFileProvider::new(temp_dir.path().to_path_buf());

        assert!(result.is_ok());
    }

    #[test]
    fn test_new_with_nonexistent_directory() {
        let result = FilesystemBootFileProvider::new(PathBuf::from("/nonexistent/path/12345"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_new_with_file_instead_of_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file.txt");
        std::fs::File::create(&file_path).unwrap();

        let result = FilesystemBootFileProvider::new(file_path);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[tokio::test]
    async fn test_get_file_success() {
        let (provider, _temp_dir) = create_test_provider();

        let result = provider.get_file("snponly.efi").await;

        assert!(result.is_ok());
        let mut reader = result.unwrap();
        let mut contents = Vec::new();
        reader.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"IPXE_EFI_BINARY_DATA");
    }

    #[tokio::test]
    async fn test_get_file_second_file() {
        let (provider, _temp_dir) = create_test_provider();

        let result = provider.get_file("undionly.kpxe").await;

        assert!(result.is_ok());
        let mut reader = result.unwrap();
        let mut contents = Vec::new();
        reader.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"KPXE_BINARY");
    }

    #[tokio::test]
    async fn test_get_file_exists_within_base_dir() {
        let (provider, _temp_dir) = create_test_provider();

        let result = provider.get_file("unauthorized.bin").await;

        // File exists but is accessible since it's within base directory
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_file_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FilesystemBootFileProvider::new(temp_dir.path().to_path_buf()).unwrap();

        let result = provider.get_file("nonexistent.efi").await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Failed to access boot file"));
    }

    #[tokio::test]
    async fn test_filesize_success() {
        let (provider, _temp_dir) = create_test_provider();

        let result = BootFileProvider::filesize(&provider, "snponly.efi").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"IPXE_EFI_BINARY_DATA".len() as u64);
    }

    #[tokio::test]
    async fn test_filesize_second_file() {
        let (provider, _temp_dir) = create_test_provider();

        let result = BootFileProvider::filesize(&provider, "undionly.kpxe").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"KPXE_BINARY".len() as u64);
    }

    #[tokio::test]
    async fn test_filesize_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FilesystemBootFileProvider::new(temp_dir.path().to_path_buf()).unwrap();

        let result = BootFileProvider::filesize(&provider, "nonexistent.efi").await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Failed to access boot file"));
    }

    #[tokio::test]
    async fn test_path_traversal_attack_blocked() {
        let (provider, temp_dir) = create_test_provider();

        // Try to access a file outside the base directory using path traversal
        // Even if we create a file in parent directory
        let parent_dir = temp_dir.path().parent().unwrap();
        let attack_file = parent_dir.join("secret.txt");
        std::fs::File::create(&attack_file)
            .unwrap()
            .write_all(b"SECRET")
            .unwrap();

        // Attempt path traversal - should be blocked by canonicalization
        let result = provider.get_file("../secret.txt").await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("outside boot files directory"));

        let result = BootFileProvider::filesize(&provider, "../secret.txt").await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("outside boot files directory"));

        // Clean up
        std::fs::remove_file(&attack_file).unwrap();
    }

    #[tokio::test]
    async fn test_subdirectory_access_allowed() {
        let temp_dir = TempDir::new().unwrap();

        // Create a subdirectory with a file
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let file_path = subdir.join("nested.efi");
        std::fs::File::create(&file_path)
            .unwrap()
            .write_all(b"NESTED_DATA")
            .unwrap();

        let provider = FilesystemBootFileProvider::new(temp_dir.path().to_path_buf()).unwrap();

        // Should be able to access files in subdirectories
        let result = provider.get_file("subdir/nested.efi").await;
        assert!(result.is_ok());
        let mut reader = result.unwrap();
        let mut contents = Vec::new();
        reader.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"NESTED_DATA");
    }

    #[tokio::test]
    async fn test_symlink_escape_blocked() {
        let (provider, temp_dir) = create_test_provider();

        // Create a file outside the base directory
        let parent_dir = temp_dir.path().parent().unwrap();
        let outside_file = parent_dir.join("outside.txt");
        std::fs::File::create(&outside_file)
            .unwrap()
            .write_all(b"OUTSIDE")
            .unwrap();

        // Create a symlink inside base directory pointing outside
        let symlink_path = temp_dir.path().join("escape.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_file, &symlink_path).ok();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside_file, &symlink_path).ok();

        // Attempt to access via symlink - should be blocked after canonicalization
        let result = provider.get_file("escape.txt").await;
        if symlink_path.exists() {
            // Symlink was created successfully
            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(error_msg.contains("outside boot files directory"));
        }

        // Clean up
        std::fs::remove_file(&outside_file).ok();
        std::fs::remove_file(&symlink_path).ok();
    }
}
