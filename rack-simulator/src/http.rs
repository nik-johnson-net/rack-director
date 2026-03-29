pub fn parse_ipxe_script(script: &str) -> IpxeScript {
    let mut result = IpxeScript::default();

    for line in script.lines() {
        let line = line.trim();

        if line.starts_with("kernel ") {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                result.kernel_url = Some(parts[1].to_string());
            }
            if parts.len() >= 3 {
                result.cmdline = Some(parts[2].to_string());
            }
        } else if line.starts_with("initrd ") {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                result.initrd_url = Some(parts[1].to_string());
            }
        } else if line.starts_with("chain ") {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                result.chain_url = Some(parts[1].to_string());
            }
        } else if line.starts_with("sanboot ") {
            result.is_sanboot = true;
        } else if line.starts_with("exit") {
            result.is_exit = true;
        } else if line.starts_with("reboot") {
            result.is_reboot = true;
        }
    }

    result
}

#[derive(Debug, Default)]
pub struct IpxeScript {
    pub kernel_url: Option<String>,
    pub initrd_url: Option<String>,
    pub cmdline: Option<String>,
    pub chain_url: Option<String>,
    pub is_sanboot: bool,
    pub is_exit: bool,
    pub is_reboot: bool,
}
