use anyhow::Result;
use dhcproto::v4::{self, Architecture, Message};
use std::net::Ipv4Addr;
use std::sync::Arc;

use crate::boot_files::BootFileProvider;

use super::request::RequestContext;

#[derive(Debug, Clone)]
struct BootOptions {
    next_server: Option<String>,
    filename: String,
    file_size_blocks: Option<u16>,
}

#[derive(Clone)]
pub struct BootConfigProvider {
    tftp_server: String,
    http_server: String,
    boot_file_provider: Arc<dyn BootFileProvider>,
}

impl BootConfigProvider {
    /// Creates a new BootConfigProvider.
    ///
    /// # Arguments
    /// * `tftp_server` - TFTP server address (IP or hostname)
    /// * `http_server` - HTTP server URL (must start with `http://` or `https://`)
    /// * `boot_file_provider` - Provider for accessing boot files and their sizes
    ///
    /// # Panics
    /// Panics if `http_server` does not start with `http://` or `https://`.
    /// This validation ensures boot scripts always receive valid HTTP URLs.
    pub fn new(
        tftp_server: String,
        http_server: String,
        boot_file_provider: Arc<dyn BootFileProvider>,
    ) -> Self {
        // Validate that http_server has proper URL scheme
        if !http_server.starts_with("http://") && !http_server.starts_with("https://") {
            panic!(
                "http_server must start with 'http://' or 'https://', got: '{}'",
                http_server
            );
        }

        Self {
            tftp_server,
            http_server,
            boot_file_provider,
        }
    }

    /// Resolves and applies boot options to a DHCP message.
    ///
    /// Decision logic (in order):
    /// 1. Check if client requested any boot options — skip if not requested
    /// 2. iPXE client → filename = HTTP boot script URL (no file size)
    /// 3. HTTP boot arch (15/16/17) → filename = HTTP URL for iPXE firmware
    /// 4. UEFI arch (7, 11) → next_server = TFTP server, filename = snponly.efi
    /// 5. BIOS arch (0, 9, default) → next_server = TFTP server, filename = undionly.kpxe
    ///
    /// For actual boot files (not scripts), looks up file size and includes Option 13 if requested.
    pub async fn populate_boot_options(
        &self,
        msg: &mut Message,
        req_ctx: &RequestContext,
    ) -> Result<()> {
        // 1. Check if client requested any boot options
        if !req_ctx.requested_tftp_server && !req_ctx.requested_bootfile {
            log::debug!("Client did not request any boot options");
            return Ok(());
        }

        // 2. iPXE client → HTTP boot script (no file to lookup)
        if req_ctx.is_ipxe {
            let boot_opts = BootOptions {
                next_server: None,
                filename: format!("{}/cnc/ipxe", self.http_server),
                file_size_blocks: None, // Boot scripts have no file size
            };
            self.apply_boot_options_to_message(msg, &boot_opts, req_ctx)?;
            return Ok(());
        }

        // 3. Determine boot file and lookup size
        log::debug!(
            "DHCP: Matching boot args to client arch {:?}",
            req_ctx.client_arch
        );
        let boot_opts = match req_ctx.client_arch {
            // HTTP Boot architectures (15/16/17) → HTTP URL for iPXE firmware
            Some(
                Architecture::Unknown(15) | Architecture::Unknown(16) | Architecture::Unknown(17),
            ) => {
                let filename = "snponly.efi";
                let file_size_blocks = self.lookup_file_size_blocks(filename).await;
                BootOptions {
                    next_server: None,
                    filename: format!("{}/cnc/boot/snponly.efi", self.http_server),
                    file_size_blocks,
                }
            }
            // UEFI architectures (7, 11) → TFTP snponly.efi
            // NOTE: Bug in library mis-maps BC to 7, and x86_64 to 9. The spec has been amended for x86_64 to be 7.
            Some(Architecture::BC | Architecture::Unknown(11)) => {
                let filename = "snponly.efi";
                let file_size_blocks = self.lookup_file_size_blocks(filename).await;
                BootOptions {
                    next_server: Some(self.tftp_server.clone()),
                    filename: filename.to_string(),
                    file_size_blocks,
                }
            }
            // BIOS architectures (0, 9) and default → TFTP undionly.kpxe
            _ => {
                let filename = "undionly.kpxe";
                let file_size_blocks = self.lookup_file_size_blocks(filename).await;
                BootOptions {
                    next_server: Some(self.tftp_server.clone()),
                    filename: filename.to_string(),
                    file_size_blocks,
                }
            }
        };

        self.apply_boot_options_to_message(msg, &boot_opts, req_ctx)?;
        Ok(())
    }

    /// Looks up the file size in 512-byte blocks for a boot file.
    ///
    /// Returns None if the file cannot be found or if there's an error accessing it.
    /// Logs a warning on error but does not fail the DHCP response.
    ///
    /// # Arguments
    /// * `filename` - The boot file name (e.g., "snponly.efi", "undionly.kpxe")
    ///
    /// # Returns
    /// * `Some(blocks)` - File size in 512-byte blocks (rounded up)
    /// * `None` - File not found or error occurred
    async fn lookup_file_size_blocks(&self, filename: &str) -> Option<u16> {
        match self.boot_file_provider.filesize(filename).await {
            Ok(size_bytes) => {
                // Calculate blocks: round up (size_bytes + 511) / 512
                let blocks = size_bytes.div_ceil(512) as u16;
                log::debug!(
                    "Boot file '{}': {} bytes = {} blocks",
                    filename,
                    size_bytes,
                    blocks
                );
                Some(blocks)
            }
            Err(e) => {
                log::warn!(
                    "Failed to get file size for '{}': {}. Option 13 will be omitted.",
                    filename,
                    e
                );
                None
            }
        }
    }

    /// Applies boot options to a DHCP message.
    ///
    /// This method selectively adds DHCP options based on what the client requested:
    /// - Option 66: TFTP Server Name - only if requested AND next_server is Some
    /// - Option 67: Bootfile Name - only if requested
    /// - Option 13: Boot File Size - only if requested AND file_size_blocks is Some
    /// - siaddr field: Next server IP address - only if option 66 requested
    ///
    /// # Arguments
    /// * `msg` - The DHCP message to modify
    /// * `boot_opts` - The boot options to apply
    /// * `req_ctx` - The request context containing client option requests
    ///
    /// # Returns
    /// * `Ok(())` - Options applied successfully
    /// * `Err(_)` - Failed to parse next_server IP address
    fn apply_boot_options_to_message(
        &self,
        msg: &mut Message,
        boot_opts: &BootOptions,
        req_ctx: &RequestContext,
    ) -> Result<()> {
        // Option 66 (TFTP Server Name) - only if requested and applicable
        if req_ctx.requested_tftp_server
            && let Some(next_server) = &boot_opts.next_server
        {
            msg.opts_mut().insert(v4::DhcpOption::TFTPServerName(
                next_server.clone().into_bytes(),
            ));
        }

        // Option 67 (Bootfile Name) - only if requested
        if req_ctx.requested_bootfile {
            msg.opts_mut().insert(v4::DhcpOption::BootfileName(
                boot_opts.filename.clone().into_bytes(),
            ));
        }

        // Option 13 (Boot File Size) - only if requested AND we have a file size
        if req_ctx.requested_bootfile_size
            && let Some(blocks) = boot_opts.file_size_blocks
        {
            msg.opts_mut().insert(v4::DhcpOption::BootFileSize(blocks));
        }

        // siaddr field (next server IP) - only if option 66 requested
        if req_ctx.requested_tftp_server
            && let Some(next_server) = &boot_opts.next_server
            && let Ok(next_ip) = next_server.parse::<Ipv4Addr>()
        {
            msg.set_siaddr(next_ip);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use dhcproto::v4::{Message, MessageType, Opcode};
    use std::collections::HashMap;
    use std::net::Ipv4Addr;
    use std::sync::Mutex as StdMutex;
    use tokio::io::BufReader;

    /// Mock BootFileProvider for testing
    struct MockBootFileProvider {
        file_sizes: StdMutex<HashMap<String, Result<u64, String>>>,
    }

    impl MockBootFileProvider {
        fn new() -> Self {
            Self {
                file_sizes: StdMutex::new(HashMap::new()),
            }
        }

        fn with_file(self, filename: &str, size: u64) -> Self {
            self.file_sizes
                .lock()
                .unwrap()
                .insert(filename.to_string(), Ok(size));
            self
        }

        fn with_error(self, filename: &str, error: &str) -> Self {
            self.file_sizes
                .lock()
                .unwrap()
                .insert(filename.to_string(), Err(error.to_string()));
            self
        }
    }

    #[async_trait]
    impl BootFileProvider for MockBootFileProvider {
        async fn get_file(&self, _filename: &str) -> Result<BufReader<tokio::fs::File>> {
            anyhow::bail!("get_file not implemented in mock")
        }

        async fn filesize(&self, filename: &str) -> Result<u64> {
            let sizes = self.file_sizes.lock().unwrap();
            match sizes.get(filename) {
                Some(Ok(size)) => Ok(*size),
                Some(Err(e)) => anyhow::bail!("{}", e),
                None => anyhow::bail!("File not found: {}", filename),
            }
        }
    }

    fn make_req_ctx(
        client_arch: Option<Architecture>,
        is_ipxe: bool,
        requested_tftp_server: bool,
        requested_bootfile: bool,
        requested_bootfile_size: bool,
    ) -> RequestContext {
        RequestContext {
            mac: "aa:bb:cc:dd:ee:ff".to_string(),
            message_type: MessageType::Discover,
            requested_ip: None,
            client_arch,
            is_ipxe,
            requested_tftp_server,
            requested_bootfile,
            requested_bootfile_size,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            guid: None,
        }
    }

    fn make_provider() -> BootConfigProvider {
        let mock = MockBootFileProvider::new()
            .with_file("snponly.efi", 1024000) // ~1MB
            .with_file("undionly.kpxe", 102400); // ~100KB
        BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        )
    }

    fn get_bootfile_name(msg: &Message) -> Option<String> {
        msg.opts().iter().find_map(|(_, opt)| {
            if let v4::DhcpOption::BootfileName(name) = opt {
                Some(String::from_utf8_lossy(name).to_string())
            } else {
                None
            }
        })
    }

    fn get_tftp_server_name(msg: &Message) -> Option<String> {
        msg.opts().iter().find_map(|(_, opt)| {
            if let v4::DhcpOption::TFTPServerName(name) = opt {
                Some(String::from_utf8_lossy(name).to_string())
            } else {
                None
            }
        })
    }

    fn get_bootfile_size(msg: &Message) -> Option<u16> {
        msg.opts().iter().find_map(|(_, opt)| {
            if let v4::DhcpOption::BootFileSize(size) = opt {
                Some(*size)
            } else {
                None
            }
        })
    }

    // URL validation tests
    #[test]
    fn test_new_with_valid_http_url() {
        let mock = MockBootFileProvider::new();
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        );
        assert_eq!(provider.http_server, "http://10.0.0.1");
    }

    #[test]
    fn test_new_with_valid_https_url() {
        let mock = MockBootFileProvider::new();
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "https://10.0.0.1".to_string(),
            Arc::new(mock),
        );
        assert_eq!(provider.http_server, "https://10.0.0.1");
    }

    #[test]
    fn test_new_with_valid_http_url_with_port() {
        let mock = MockBootFileProvider::new();
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1:3000".to_string(),
            Arc::new(mock),
        );
        assert_eq!(provider.http_server, "http://10.0.0.1:3000");
    }

    #[test]
    #[should_panic(expected = "http_server must start with 'http://' or 'https://'")]
    fn test_new_with_invalid_url_no_scheme() {
        let mock = MockBootFileProvider::new();
        BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "10.0.0.1".to_string(),
            Arc::new(mock),
        );
    }

    #[test]
    #[should_panic(expected = "http_server must start with 'http://' or 'https://'")]
    fn test_new_with_invalid_url_wrong_scheme() {
        let mock = MockBootFileProvider::new();
        BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "ftp://10.0.0.1".to_string(),
            Arc::new(mock),
        );
    }

    // iPXE client tests
    #[tokio::test]
    async fn test_populate_boot_options_ipxe_client() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(None, true, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/ipxe".to_string()),
            "iPXE client should get HTTP URL for boot script"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "iPXE client should not have TFTP server set"
        );
        assert_eq!(
            msg.siaddr(),
            Ipv4Addr::UNSPECIFIED,
            "iPXE client should not have siaddr set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "iPXE boot script should not have file size (not a file)"
        );
    }

    // HTTP Boot architecture tests (14/15/16)
    #[tokio::test]
    async fn test_populate_boot_options_http_boot_arch_17() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Unknown(17)), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/boot/snponly.efi".to_string()),
            "Arch 14 should get HTTP URL for iPXE firmware"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "HTTP boot should not have TFTP server set"
        );
        assert_eq!(
            msg.siaddr(),
            Ipv4Addr::UNSPECIFIED,
            "HTTP boot should not have siaddr set"
        );
        // File size should be provided: 1024000 bytes = 2000 blocks
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "HTTP boot should have file size for snponly.efi"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_http_boot_arch_15() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Unknown(15)), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/boot/snponly.efi".to_string()),
            "Arch 15 should get HTTP URL for iPXE firmware"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "HTTP boot should not have TFTP server set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "HTTP boot should have file size for snponly.efi"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_http_boot_arch_16() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Unknown(16)), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/boot/snponly.efi".to_string()),
            "Arch 16 should get HTTP URL for iPXE firmware"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "HTTP boot should not have TFTP server set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "HTTP boot should have file size for snponly.efi"
        );
    }

    // UEFI architecture tests (7, 11)
    #[tokio::test]
    async fn test_populate_boot_options_uefi_arch_7() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("snponly.efi".to_string()),
            "UEFI arch 7 should get snponly.efi"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "UEFI arch 7 should have TFTP server set"
        );
        assert_eq!(
            msg.siaddr(),
            "10.0.0.1".parse::<Ipv4Addr>().unwrap(),
            "UEFI arch 7 should have siaddr set to TFTP server"
        );
        // File size should be provided: 1024000 bytes = 2000 blocks
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "UEFI arch 7 should have file size for snponly.efi"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_uefi_arch_11() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Unknown(11)), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("snponly.efi".to_string()),
            "UEFI arch 11 should get snponly.efi"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "UEFI arch 11 should have TFTP server set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "UEFI arch 11 should have file size for snponly.efi"
        );
    }

    // BIOS architecture tests (0, 9, default)
    #[tokio::test]
    async fn test_populate_boot_options_bios_arch_0() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Intelx86PC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("undionly.kpxe".to_string()),
            "BIOS arch 0 should get undionly.kpxe"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "BIOS arch 0 should have TFTP server set"
        );
        assert_eq!(
            msg.siaddr(),
            "10.0.0.1".parse::<Ipv4Addr>().unwrap(),
            "BIOS arch 0 should have siaddr set to TFTP server"
        );
        // File size should be provided: 102400 bytes = 200 blocks
        assert_eq!(
            get_bootfile_size(&msg),
            Some(200),
            "BIOS arch 0 should have file size for undionly.kpxe"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_bios_arch_9() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::X86_64), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("undionly.kpxe".to_string()),
            "BIOS arch 9 should get undionly.kpxe"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "BIOS arch 9 should have TFTP server set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(200),
            "BIOS arch 9 should have file size for undionly.kpxe"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_no_arch_default_bios() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(None, false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("undionly.kpxe".to_string()),
            "Default (no arch) should get undionly.kpxe (BIOS)"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "Default (no arch) should have TFTP server set"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(200),
            "Default (no arch) should have file size for undionly.kpxe"
        );
    }

    // Selective option request tests
    #[tokio::test]
    async fn test_populate_boot_options_only_tftp_server_requested() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, false, false);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_tftp_server_name(&msg),
            Some("10.0.0.1".to_string()),
            "Should have TFTP server set when only option 66 requested"
        );
        assert_eq!(
            get_bootfile_name(&msg),
            None,
            "Should NOT have bootfile name when option 67 not requested"
        );
        assert_eq!(
            msg.siaddr(),
            "10.0.0.1".parse::<Ipv4Addr>().unwrap(),
            "Should have siaddr set when option 66 requested"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "Should NOT have file size when option 13 not requested"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_only_bootfile_requested() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, false, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("snponly.efi".to_string()),
            "Should have bootfile name when only option 67 requested"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "Should NOT have TFTP server when option 66 not requested"
        );
        assert_eq!(
            msg.siaddr(),
            Ipv4Addr::UNSPECIFIED,
            "Should NOT have siaddr set when option 66 not requested"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "Should have file size when option 13 requested"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_http_boot_only_bootfile_applicable() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::Unknown(15)), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/boot/snponly.efi".to_string()),
            "HTTP boot should return bootfile name when requested"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "HTTP boot should NOT return TFTP server (not applicable)"
        );
        assert_eq!(
            msg.siaddr(),
            Ipv4Addr::UNSPECIFIED,
            "HTTP boot should NOT set siaddr (not applicable)"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2000),
            "HTTP boot should have file size for snponly.efi"
        );
    }

    #[tokio::test]
    async fn test_populate_boot_options_no_options_requested() {
        let provider = make_provider();
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, false, false, false);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            None,
            "Should NOT have bootfile name when no options requested"
        );
        assert_eq!(
            get_tftp_server_name(&msg),
            None,
            "Should NOT have TFTP server when no options requested"
        );
        assert_eq!(
            msg.siaddr(),
            Ipv4Addr::UNSPECIFIED,
            "Should NOT have siaddr set when no options requested"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "Should NOT have file size when no options requested"
        );
    }

    // Option 13 (Boot File Size) specific tests
    #[tokio::test]
    async fn test_option_13_size_calculation_exact_blocks() {
        let mock = MockBootFileProvider::new().with_file("snponly.efi", 512);
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        );
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_size(&msg),
            Some(1),
            "512 bytes should be 1 block"
        );
    }

    #[tokio::test]
    async fn test_option_13_size_calculation_round_up() {
        let mock = MockBootFileProvider::new()
            .with_file("snponly.efi", 600)
            .with_file("undionly.kpxe", 1025);
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        );

        // Test 600 bytes = 2 blocks (rounds up from 1.17)
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();
        assert_eq!(
            get_bootfile_size(&msg),
            Some(2),
            "600 bytes should be 2 blocks"
        );

        // Test 1025 bytes = 3 blocks (rounds up from 2.002)
        let req_ctx = make_req_ctx(Some(Architecture::Intelx86PC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();
        assert_eq!(
            get_bootfile_size(&msg),
            Some(3),
            "1025 bytes should be 3 blocks"
        );
    }

    #[tokio::test]
    async fn test_option_13_omitted_when_not_requested() {
        let provider = make_provider();
        // Request bootfile but NOT file size
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, false);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("snponly.efi".to_string()),
            "Should have bootfile name"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "Should NOT have file size when option 13 not requested"
        );
    }

    #[tokio::test]
    async fn test_option_13_omitted_on_file_lookup_error() {
        let mock = MockBootFileProvider::new().with_error("snponly.efi", "File not found");
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        );
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("snponly.efi".to_string()),
            "Should have bootfile name even if size lookup fails"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "Should NOT have file size when lookup fails"
        );
    }

    #[tokio::test]
    async fn test_option_13_omitted_for_boot_scripts() {
        let provider = make_provider();
        // iPXE client gets boot script (no physical file)
        let req_ctx = make_req_ctx(None, true, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);

        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();

        assert_eq!(
            get_bootfile_name(&msg),
            Some("http://10.0.0.1/cnc/ipxe".to_string()),
            "iPXE should get boot script URL"
        );
        assert_eq!(
            get_bootfile_size(&msg),
            None,
            "Boot scripts should NOT have file size (not a physical file)"
        );
    }

    #[tokio::test]
    async fn test_option_13_multiple_architectures() {
        let mock = MockBootFileProvider::new()
            .with_file("snponly.efi", 1024000) // 2000 blocks
            .with_file("undionly.kpxe", 102400); // 200 blocks
        let provider = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            Arc::new(mock),
        );

        // UEFI gets snponly.efi
        let req_ctx = make_req_ctx(Some(Architecture::BC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();
        assert_eq!(get_bootfile_name(&msg), Some("snponly.efi".to_string()));
        assert_eq!(get_bootfile_size(&msg), Some(2000));

        // BIOS gets undionly.kpxe
        let req_ctx = make_req_ctx(Some(Architecture::Intelx86PC), false, true, true, true);
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        provider
            .populate_boot_options(&mut msg, &req_ctx)
            .await
            .unwrap();
        assert_eq!(get_bootfile_name(&msg), Some("undionly.kpxe".to_string()));
        assert_eq!(get_bootfile_size(&msg), Some(200));
    }
}
