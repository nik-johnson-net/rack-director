use axum::{
    http::{StatusCode, header},
    response::Response,
};

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

/// Generates an iPXE script that boots a custom kernel with ramdisk.
///
/// This script instructs iPXE to download and boot a custom Linux kernel and initramfs
/// from the HTTP server. Used for device discovery, OS installation, or maintenance tasks.
///
/// # Arguments
/// * `ramdisk` - The filename of the initramfs image
/// * `kernel` - The filename of the kernel image
/// * `cmdline` - Kernel command line arguments
pub fn generate_kernel_script(ramdisk: &str, kernel: &str, cmdline: &str) -> String {
    format!(
        r#"#!ipxe
# Boot custom linux image for new device intake
kernel {kernel} {cmdline}
initrd {ramdisk}
boot
"#
    )
}

/// Generates an iPXE script that redirects to the main iPXE endpoint with UUID and MAC.
///
/// This script is sent to devices that boot without providing their UUID. It instructs
/// iPXE to chain-load back to the iPXE endpoint, this time including the device's UUID
/// and MAC address as query parameters.
///
/// # Arguments
/// * `root_url` - The base HTTP URL of the rack-director server
pub fn generate_uuid_script(root_url: &str) -> String {
    format!(
        r#"#!ipxe
# Chain boot to send uuid and mac
chain {root_url}/cnc/ipxe?uuid=${{uuid}}&mac=${{netX/mac}}
"#
    )
}

/// Generates an iPXE redirect response with UUID collection script.
///
/// This is a convenience wrapper around `generate_uuid_script` that returns a complete
/// HTTP response ready to be sent to the client.
///
/// # Arguments
/// * `root_url` - The base HTTP URL of the rack-director server
pub fn generate_uuid_redirect(root_url: &str) -> Response<String> {
    build_response(generate_uuid_script(root_url))
}

/// Builds an HTTP response containing an iPXE script.
///
/// Creates a 200 OK response with Content-Type: text/plain containing the provided
/// iPXE script. iPXE expects plain text responses.
///
/// # Arguments
/// * `script` - The complete iPXE script content
pub fn build_response(script: String) -> Response<String> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(script)
        .expect("response building should never error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_boot_local_script() {
        let script = generate_boot_local_script();
        assert!(script.contains("#!ipxe"));
        assert!(script.contains("exit"));
    }

    #[test]
    fn test_generate_kernel_script() {
        let script = generate_kernel_script(
            "http://example.com/cnc/images/initramfs.img",
            "http://example.com/cnc/images/vmlinuz",
            "console=ttyS0",
        );
        assert!(script.contains("#!ipxe"));
        assert!(script.contains("kernel http://example.com/cnc/images/vmlinuz console=ttyS0"));
        assert!(script.contains("initrd http://example.com/cnc/images/initramfs.img"));
        assert!(script.contains("boot"));
    }

    #[test]
    fn test_generate_uuid_script() {
        let script = generate_uuid_script("http://example.com");
        assert!(script.contains("#!ipxe"));
        assert!(script.contains("chain http://example.com/cnc/ipxe?uuid=${uuid}&mac=${netX/mac}"));
    }

    #[test]
    fn test_generate_uuid_redirect() {
        let response = generate_uuid_redirect("http://example.com");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        let body = response.into_body();
        assert!(body.contains("chain http://example.com/cnc/ipxe?uuid=${uuid}&mac=${netX/mac}"));
    }

    #[test]
    fn test_build_response() {
        let script = "#!ipxe\nboot\n".to_string();
        let response = build_response(script.clone());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        assert_eq!(response.into_body(), script);
    }
}
