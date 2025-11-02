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
                // BIOS: Use TFTP to download undionly.kpxe (iPXE for BIOS)
                // Then iPXE will fetch the boot script via HTTP
                next_server: self.tftp_server.clone(),
                filename: "undionly.kpxe".to_string(),
            }),
            BootMode::UefiBoot => Ok(BootOptions {
                // UEFI x86-64: Use TFTP to download ipxe.efi (iPXE for UEFI)
                // Then iPXE will fetch the boot script via HTTP
                next_server: self.tftp_server.clone(),
                filename: "ipxe.efi".to_string(),
            }),
            BootMode::UefiArm64 => Ok(BootOptions {
                // UEFI ARM64: Use TFTP to download ipxe.efi (iPXE for UEFI ARM64)
                // Then iPXE will fetch the boot script via HTTP
                next_server: self.tftp_server.clone(),
                filename: "ipxe.efi".to_string(),
            }),
        }
    }

    pub fn get_ipxe_boot_script(&self) -> Result<BootOptions> {
        // iPXE second-stage: Return HTTP URL for boot script
        // This is called when iPXE makes a second DHCP request after booting
        Ok(BootOptions {
            next_server: self.http_server.clone(),
            filename: format!("http://{}/cnc/ipxe", self.http_server),
        })
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
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_uefi_arm64_boot_options() {
        let provider = BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string());
        let opts = provider.get_boot_options(BootMode::UefiArm64).unwrap();
        assert_eq!(opts.next_server, "10.0.0.1");
        assert_eq!(opts.filename, "ipxe.efi");
    }

    #[test]
    fn test_ipxe_boot_script() {
        let provider = BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1:3000".to_string());
        let opts = provider.get_ipxe_boot_script().unwrap();
        assert_eq!(opts.next_server, "10.0.0.1:3000");
        assert_eq!(opts.filename, "http://10.0.0.1:3000/cnc/ipxe");
    }
}
