use anyhow::Result;

use crate::templates;

#[derive(Debug)]
pub enum BootTarget {
    LocalDisk,
    /// Sleep for the given number of seconds and then reboot.
    ///
    /// Used when a device has no active plan and is not yet provisioned,
    /// so it retries PXE boot after a delay rather than falling through to
    /// local disk (which may not have a bootable OS).
    SleepReboot {
        seconds: u64,
    },
    AgentImage {
        action: String,
        cmdline: String,
    },
    NetBoot {
        ramdisk: String,
        kernel: String,
        modules: Vec<String>,
        cmdline: String,
    },
}

impl BootTarget {
    /// Generate an iPXE script for this boot target.
    ///
    /// Storage paths stored in `NetBoot` variants use the format
    /// `osm/{module}/{version}/{os_dir}/{file}`.  The `/cnc/` prefix is
    /// prepended here to produce the full URL that iPXE will fetch.
    pub async fn to_ipxe_script(
        &self,
        root_url: &str,
        device_uuid: Option<&uuid::Uuid>,
    ) -> Result<String> {
        match self {
            BootTarget::LocalDisk => Ok(generate_boot_local_script()),
            BootTarget::SleepReboot { seconds } => Ok(generate_sleep_reboot_script(*seconds)),
            BootTarget::AgentImage { action, cmdline } => {
                let full_cmdline = format!(
                    "{} rackdirector.action={} rackdirector.url={}",
                    cmdline, action, root_url
                );

                // Agent Images are shipped with rack-director and not stored in the ImageStore.
                let kernel = format!("{}/cnc/agent-images/vmlinuz", root_url);
                let initramfs = format!("{}/cnc/agent-images/initramfs.img", root_url);

                let script =
                    generate_netboot_script(&kernel, &initramfs, &full_cmdline, &Vec::new());
                Ok(script)
            }
            BootTarget::NetBoot {
                ramdisk,
                kernel,
                modules,
                cmdline,
            } => {
                // Storage paths are like "osm/{module}/{version}/{os_dir}/{file}".
                // Prepend "{root_url}/cnc/" to produce the full serving URL.
                let kernel_url = format!("{}/cnc/{}", root_url, kernel);
                let initrd_url = format!("{}/cnc/{}", root_url, ramdisk);
                let module_urls: Vec<String> = modules
                    .iter()
                    .map(|m| format!("{}/cnc/{}", root_url, m))
                    .collect();

                // Run template.
                let resolved_cmdline =
                    templates::render_cmdline_args(cmdline, root_url, device_uuid)?;

                Ok(generate_netboot_script(
                    &kernel_url,
                    &initrd_url,
                    &resolved_cmdline,
                    &module_urls,
                ))
            }
        }
    }
}

/// Generates an iPXE script that boots from the local disk.
///
/// This script instructs iPXE to boot from the first hard disk (0x80 in BIOS numbering).
/// Used for devices that should boot from their locally installed OS.
pub fn generate_boot_local_script() -> String {
    r#"#!ipxe
# Boot to local disk for known device
exit
"#
    .to_string()
}

/// Generates an iPXE script that sleeps for the given number of seconds and then reboots.
///
/// Used when a device has no active plan and has not yet been provisioned. Rather than
/// falling through to local disk (which may not have a bootable OS), the device waits
/// and retries PXE boot so it will pick up a plan when one becomes available.
pub fn generate_sleep_reboot_script(seconds: u64) -> String {
    format!(
        r#"#!ipxe
# No active plan - sleep and retry
sleep {seconds}
reboot
"#
    )
}

pub fn generate_netboot_script(
    kernel: &str,
    initrd: &str,
    cmdline: &str,
    modules: &[String],
) -> String {
    let module_line = if modules.is_empty() {
        String::new()
    } else {
        format!("module {}", modules.join(" "))
    };
    format!(
        r#"#!ipxe
# Boot custom linux image for new device intake
kernel {kernel} {cmdline}
initrd {initrd}
{module_line}
boot
"#
    )
}

#[cfg(test)]
mod tests {
    use crate::plans::actions::boot_target::{
        generate_netboot_script, generate_sleep_reboot_script,
    };

    #[test]
    fn sleep_reboot_script_contains_sleep_and_reboot() {
        let script = generate_sleep_reboot_script(600);
        assert!(script.starts_with("#!ipxe\n"));
        assert!(script.contains("sleep 600\n"));
        assert!(script.contains("reboot\n"));
    }

    #[test]
    fn sleep_reboot_script_zero_seconds() {
        let script = generate_sleep_reboot_script(0);
        assert!(script.contains("sleep 0\n"));
        assert!(script.contains("reboot\n"));
    }

    #[test]
    fn sleep_reboot_script_exact_output() {
        let expected = "#!ipxe\n# No active plan - sleep and retry\nsleep 300\nreboot\n";
        assert_eq!(generate_sleep_reboot_script(300), expected);
    }

    #[test]
    fn netboot_script_no_modules() {
        let expected = r#"#!ipxe
# Boot custom linux image for new device intake
kernel vmlinuz opt1 opt2
initrd initramfs.img

boot
"#;
        assert_eq!(
            generate_netboot_script("vmlinuz", "initramfs.img", "opt1 opt2", &[]),
            expected
        );
    }

    #[test]
    fn netboot_script_with_modules() {
        let expected = r#"#!ipxe
# Boot custom linux image for new device intake
kernel vmlinuz opt1 opt2
initrd initramfs.img
module mod1.ko mod2.ko
boot
"#;
        assert_eq!(
            generate_netboot_script(
                "vmlinuz",
                "initramfs.img",
                "opt1 opt2",
                &["mod1.ko".to_owned(), "mod2.ko".to_owned()]
            ),
            expected
        );
    }
}
