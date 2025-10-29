use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    BiosLegacy,
    UefiBoot,
    UefiArm64,
}

#[derive(Debug, Clone)]
pub struct BootOptions {
    pub next_server: String,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct BootConfigProvider {
    tftp_server: String,
    http_server: String,
}

impl BootConfigProvider {
    pub fn new(tftp_server: String, http_server: String) -> Self {
        Self {
            tftp_server,
            http_server,
        }
    }

    pub fn get_boot_options(&self, mode: BootMode) -> Result<BootOptions> {
        match mode {
            BootMode::BiosLegacy => Ok(BootOptions {
                // Use TFTP for BIOS (traditional PXE)
                next_server: self.tftp_server.clone(),
                filename: "undionly.kpxe".to_string(),
            }),
            BootMode::UefiBoot => Ok(BootOptions {
                // Use HTTP for UEFI (faster, modern)
                next_server: self.http_server.clone(),
                filename: format!("http://{}/cnc/ipxe", self.http_server),
            }),
            BootMode::UefiArm64 => Ok(BootOptions {
                next_server: self.http_server.clone(),
                filename: format!("http://{}/cnc/ipxe", self.http_server),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bios_boot_options() {
        let provider = BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::BiosLegacy).unwrap();
        assert_eq!(opts.next_server, "10.0.0.1");
        assert_eq!(opts.filename, "undionly.kpxe");
    }

    #[test]
    fn test_uefi_boot_options() {
        let provider = BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::UefiBoot).unwrap();
        assert_eq!(opts.next_server, "10.0.0.1");
        assert_eq!(opts.filename, "http://10.0.0.1/cnc/ipxe");
    }

    #[test]
    fn test_uefi_arm64_boot_options() {
        let provider = BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::UefiArm64).unwrap();
        assert_eq!(opts.next_server, "10.0.0.1");
        assert_eq!(opts.filename, "http://10.0.0.1/cnc/ipxe");
    }
}
