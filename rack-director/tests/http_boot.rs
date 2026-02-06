mod common;

use anyhow::Result;
use common::dhcp_client::{Architecture, DhcpClient};
use std::net::Ipv4Addr;

/// Test HTTP Boot flow for x86 UEFI HTTP architecture (Architecture 14)
/// Expected flow: DHCP → HTTP URL in bootfile name
#[tokio::test]
async fn test_http_boot_x86_uefi_http() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    handle.set_network_autodiscover(1, true).await?;

    // DHCP exchange for HTTP Boot client (architecture 14)
    let mac = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0x01]; // Test MAC address
    let dhcp_port = handle.handle.dhcp_port;
    let http_port = handle.handle.http_port;

    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X86UefiHttp, dhcp_port)?;
            let (offered_ip, server_id) = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, server_id)?;
            Ok((offered_ip, leased_ip, boot_options))
        })
        .await??;

    // VALIDATION: Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");
    assert!(
        !offered_ip.is_unspecified(),
        "Should receive valid IP address, got: {}",
        offered_ip
    );

    // VALIDATION: HTTP Boot - Verify DHCP response contains HTTP URL
    // Note: next_server (siaddr) may be set to DHCP server's IP, but that's OK.
    // What matters is that bootfile_name contains a full HTTP URL.
    // The URL uses the configured public URL (http://10.0.0.1) without port
    let expected_url = "http://10.0.0.1/cnc/boot/ipxe.efi";
    assert_eq!(
        boot_options.bootfile_name, expected_url,
        "Bootfile should be HTTP URL for architecture 14"
    );

    // Verify bootfile is an HTTP URL (starts with http://)
    assert!(
        boot_options.bootfile_name.starts_with("http://"),
        "HTTP Boot should provide HTTP URL in bootfile_name"
    );

    // VALIDATION: HTTP GET - Verify the bootfile can be downloaded
    // Use the actual HTTP port for downloading (127.0.0.1), not the public URL
    let http_client = reqwest::Client::new();
    let download_url = format!("http://127.0.0.1:{}/cnc/boot/ipxe.efi", http_port);
    let response = http_client.get(&download_url).send().await?;

    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP boot file download should return 200 OK"
    );

    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream"),
        "HTTP boot file should have content-type: application/octet-stream"
    );

    let downloaded_content = response.bytes().await?;

    // VALIDATION: Compare with actual fixture file
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/firmware/ipxe.efi");
    let expected_content = tokio::fs::read(fixture_path).await?;

    assert_eq!(
        downloaded_content.as_ref(),
        expected_content.as_slice(),
        "Downloaded boot file should match fixture content"
    );

    // Tests complete
    drop(handle);

    Ok(())
}

/// Test HTTP Boot flow for x64 UEFI HTTP architecture (Architecture 15)
/// Expected flow: DHCP → HTTP URL in bootfile name
#[tokio::test]
async fn test_http_boot_x64_uefi_http() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    handle.set_network_autodiscover(1, true).await?;

    // DHCP exchange for HTTP Boot client (architecture 15)
    let mac = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0x02]; // Test MAC address
    let dhcp_port = handle.handle.dhcp_port;
    let http_port = handle.handle.http_port;

    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X64UefiHttp, dhcp_port)?;
            let (offered_ip, server_id) = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, server_id)?;
            Ok((offered_ip, leased_ip, boot_options))
        })
        .await??;

    // VALIDATION: Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");
    assert!(
        !offered_ip.is_unspecified(),
        "Should receive valid IP address, got: {}",
        offered_ip
    );

    // VALIDATION: HTTP Boot - Verify DHCP response contains HTTP URL
    // Note: next_server (siaddr) may be set to DHCP server's IP, but that's OK.
    // What matters is that bootfile_name contains a full HTTP URL.
    // The URL uses the configured public URL (http://10.0.0.1) without port
    let expected_url = "http://10.0.0.1/cnc/boot/ipxe.efi";
    assert_eq!(
        boot_options.bootfile_name, expected_url,
        "Bootfile should be HTTP URL for architecture 15"
    );

    // Verify bootfile is an HTTP URL (starts with http://)
    assert!(
        boot_options.bootfile_name.starts_with("http://"),
        "HTTP Boot should provide HTTP URL in bootfile_name"
    );

    // VALIDATION: HTTP GET - Verify the bootfile can be downloaded
    let http_client = reqwest::Client::new();
    let download_url = format!("http://127.0.0.1:{}/cnc/boot/ipxe.efi", http_port);
    let response = http_client.get(&download_url).send().await?;

    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP boot file download should return 200 OK"
    );

    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream"),
        "HTTP boot file should have content-type: application/octet-stream"
    );

    let downloaded_content = response.bytes().await?;

    // VALIDATION: Compare with actual fixture file
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/firmware/ipxe.efi");
    let expected_content = tokio::fs::read(fixture_path).await?;

    assert_eq!(
        downloaded_content.as_ref(),
        expected_content.as_slice(),
        "Downloaded boot file should match fixture content"
    );

    // Tests complete
    drop(handle);

    Ok(())
}

/// Test HTTP Boot flow for ARM64 UEFI HTTP architecture (Architecture 16)
/// Expected flow: DHCP → HTTP URL in bootfile name
#[tokio::test]
async fn test_http_boot_arm64_uefi_http() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    handle.set_network_autodiscover(1, true).await?;

    // DHCP exchange for HTTP Boot client (architecture 16)
    let mac = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0x03]; // Test MAC address
    let dhcp_port = handle.handle.dhcp_port;
    let http_port = handle.handle.http_port;

    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::Arm64UefiHttp, dhcp_port)?;
            let (offered_ip, server_id) = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, server_id)?;
            Ok((offered_ip, leased_ip, boot_options))
        })
        .await??;

    // VALIDATION: Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");
    assert!(
        !offered_ip.is_unspecified(),
        "Should receive valid IP address, got: {}",
        offered_ip
    );

    // VALIDATION: HTTP Boot - Verify DHCP response contains HTTP URL
    // Note: next_server (siaddr) may be set to DHCP server's IP, but that's OK.
    // What matters is that bootfile_name contains a full HTTP URL.
    // The URL uses the configured public URL (http://10.0.0.1) without port
    let expected_url = "http://10.0.0.1/cnc/boot/ipxe.efi";
    assert_eq!(
        boot_options.bootfile_name, expected_url,
        "Bootfile should be HTTP URL for architecture 16"
    );

    // Verify bootfile is an HTTP URL (starts with http://)
    assert!(
        boot_options.bootfile_name.starts_with("http://"),
        "HTTP Boot should provide HTTP URL in bootfile_name"
    );

    // VALIDATION: HTTP GET - Verify the bootfile can be downloaded
    let http_client = reqwest::Client::new();
    let download_url = format!("http://127.0.0.1:{}/cnc/boot/ipxe.efi", http_port);
    let response = http_client.get(&download_url).send().await?;

    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP boot file download should return 200 OK"
    );

    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream"),
        "HTTP boot file should have content-type: application/octet-stream"
    );

    let downloaded_content = response.bytes().await?;

    // VALIDATION: Compare with actual fixture file
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/firmware/ipxe.efi");
    let expected_content = tokio::fs::read(fixture_path).await?;

    assert_eq!(
        downloaded_content.as_ref(),
        expected_content.as_slice(),
        "Downloaded boot file should match fixture content"
    );

    // Tests complete
    drop(handle);

    Ok(())
}

/// Regression test: Verify TFTP boot still works for x86 BIOS (Architecture 0)
/// Expected flow: DHCP → TFTP filename
#[tokio::test]
async fn test_tftp_boot_still_works_x86_bios() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    handle.set_network_autodiscover(1, true).await?;

    // DHCP exchange for traditional TFTP Boot client (architecture 0)
    let mac = [0x52, 0x54, 0x00, 0xCC, 0xDD, 0x01]; // Test MAC address
    let dhcp_port = handle.handle.dhcp_port;
    let tftp_port = handle.handle.tftp_port;

    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X86Bios, dhcp_port)?;
            let (offered_ip, server_id) = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, server_id)?;
            Ok((offered_ip, leased_ip, boot_options))
        })
        .await??;

    // VALIDATION: Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");
    assert!(
        !offered_ip.is_unspecified(),
        "Should receive valid IP address, got: {}",
        offered_ip
    );

    // VALIDATION: TFTP Boot - Verify DHCP response contains TFTP server and filename
    assert_eq!(
        boot_options.next_server,
        Ipv4Addr::new(10, 0, 0, 1),
        "TFTP Boot should have next_server set to TFTP server IP"
    );

    assert_eq!(
        boot_options.bootfile_name, "undionly.kpxe",
        "Bootfile should be TFTP filename for BIOS"
    );

    // VALIDATION: TFTP download - Verify the bootfile can be downloaded
    let bootfile_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use common::tftp_client::TftpClient;
        use std::net::SocketAddr;

        let server = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tftp_port);
        let client = TftpClient::new(server)?;
        let data = client.download("undionly.kpxe")?;
        Ok(data)
    })
    .await??;

    // VALIDATION: Compare with actual fixture file
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/firmware/undionly.kpxe");
    let expected_content = tokio::fs::read(fixture_path).await?;

    assert_eq!(
        bootfile_content, expected_content,
        "TFTP downloaded boot file should match fixture content"
    );

    // Tests complete
    drop(handle);

    Ok(())
}

/// Regression test: Verify TFTP boot still works for x64 UEFI (Architecture 7)
/// Expected flow: DHCP → TFTP filename
#[tokio::test]
async fn test_tftp_boot_still_works_x64_uefi() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    handle.set_network_autodiscover(1, true).await?;

    // DHCP exchange for traditional TFTP Boot client (architecture 7)
    let mac = [0x52, 0x54, 0x00, 0xCC, 0xDD, 0x02]; // Test MAC address
    let dhcp_port = handle.handle.dhcp_port;
    let tftp_port = handle.handle.tftp_port;

    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X64Uefi, dhcp_port)?;
            let (offered_ip, server_id) = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, server_id)?;
            Ok((offered_ip, leased_ip, boot_options))
        })
        .await??;

    // VALIDATION: Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");
    assert!(
        !offered_ip.is_unspecified(),
        "Should receive valid IP address, got: {}",
        offered_ip
    );

    // VALIDATION: TFTP Boot - Verify DHCP response contains TFTP server and filename
    assert_eq!(
        boot_options.next_server,
        Ipv4Addr::new(10, 0, 0, 1),
        "TFTP Boot should have next_server set to TFTP server IP"
    );

    assert_eq!(
        boot_options.bootfile_name, "ipxe.efi",
        "Bootfile should be TFTP filename for UEFI"
    );

    // VALIDATION: TFTP download - Verify the bootfile can be downloaded
    let bootfile_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use common::tftp_client::TftpClient;
        use std::net::SocketAddr;

        let server = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tftp_port);
        let client = TftpClient::new(server)?;
        let data = client.download("ipxe.efi")?;
        Ok(data)
    })
    .await??;

    // VALIDATION: Compare with actual fixture file
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/firmware/ipxe.efi");
    let expected_content = tokio::fs::read(fixture_path).await?;

    assert_eq!(
        bootfile_content, expected_content,
        "TFTP downloaded boot file should match fixture content"
    );

    // Tests complete
    drop(handle);

    Ok(())
}

/// Security test: Verify HTTP boot file whitelist enforcement
/// Should reject requests for files not in the whitelist
#[tokio::test]
async fn test_http_boot_file_whitelist() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;
    let http_port = handle.handle.http_port;

    let http_client = reqwest::Client::new();

    // VALIDATION: Nonexistent file should return 404
    let response = http_client
        .get(format!(
            "http://127.0.0.1:{}/cnc/boot/nonexistent.efi",
            http_port
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        404,
        "Nonexistent boot file should return 404"
    );

    // VALIDATION: Path traversal attempt should return 404
    let response = http_client
        .get(format!(
            "http://127.0.0.1:{}/cnc/boot/../../../etc/passwd",
            http_port
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        404,
        "Path traversal attempt should return 404"
    );

    // VALIDATION: Another path traversal variant (URL encoded)
    let response = http_client
        .get(format!(
            "http://127.0.0.1:{}/cnc/boot/..%2F..%2F..%2Fetc%2Fpasswd",
            http_port
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        404,
        "URL-encoded path traversal should return 404"
    );

    // VALIDATION: Unauthorized file in same directory should return 404
    let response = http_client
        .get(format!(
            "http://127.0.0.1:{}/cnc/boot/unauthorized.bin",
            http_port
        ))
        .send()
        .await?;

    assert_eq!(
        response.status().as_u16(),
        404,
        "Unauthorized file should return 404"
    );

    // Tests complete
    drop(handle);

    Ok(())
}
