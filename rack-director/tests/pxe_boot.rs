mod common;

use anyhow::Result;
use common::dhcp_client::{Architecture, DhcpClient};
use std::net::Ipv4Addr;

/// Test PXE boot flow for x86 BIOS architecture
/// Expected flow: DHCP (firmware) → TFTP (undionly.kpxe) → DHCP (iPXE) → HTTP (/cnc/ipxe)
#[tokio::test]
async fn test_pxe_boot_x86_bios() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1: First DHCP exchange (firmware requesting bootloader)
    let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]; // Test MAC address
    let dhcp_port = handle.dhcp_port;
    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X86Bios, dhcp_port)?;
            let offered_ip = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, Ipv4Addr::LOCALHOST)?;
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

    // VALIDATION: First-stage DHCP - Verify boot options for BIOS firmware
    assert_eq!(
        boot_options.next_server,
        Ipv4Addr::new(10, 0, 0, 1),
        "Next-server should be 10.0.0.1 (configured TFTP server)"
    );
    assert_eq!(
        boot_options.bootfile_name, "undionly.kpxe",
        "Bootfile should be undionly.kpxe for BIOS (iPXE bootloader)"
    );

    // Step 2: TFTP - Fetch the iPXE bootloader
    let tftp_port = handle.tftp_port;
    let bootfile_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use common::tftp_client::TftpClient;
        use std::net::SocketAddr;

        let server = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tftp_port);
        let client = TftpClient::new(server)?;
        let data = client.download("undionly.kpxe")?;
        Ok(data)
    })
    .await??;

    // VALIDATION: TFTP download - Verify we got the bootloader content
    assert!(
        !bootfile_content.is_empty(),
        "TFTP download should return non-empty bootfile"
    );
    assert_eq!(
        bootfile_content.len(),
        51,
        "TFTP bootfile should be 51 bytes (our mock undionly.kpxe)"
    );
    assert!(
        bootfile_content.starts_with(b"MOCK_IPXE_BIOS"),
        "TFTP bootfile should start with 'MOCK_IPXE_BIOS' (our test fixture)"
    );

    // Step 3: Second DHCP exchange (iPXE requesting boot script URL)
    let dhcp_port = handle.dhcp_port;
    let (leased_ip2, ipxe_boot_options) = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut dhcp_client = DhcpClient::new(mac, Architecture::X86Bios, dhcp_port)?;
        let offered_ip = dhcp_client.discover()?;
        let (leased_ip, boot_options) =
            dhcp_client.request_as_ipxe(offered_ip, Ipv4Addr::LOCALHOST)?;
        Ok((leased_ip, boot_options))
    })
    .await??;

    // VALIDATION: Second-stage DHCP - Verify iPXE gets HTTP URL
    assert_eq!(
        leased_ip2, leased_ip,
        "iPXE should receive same IP address as firmware"
    );
    // HTTP URL will use configured http_server from DHCP config (10.0.0.1)
    assert_eq!(
        ipxe_boot_options.bootfile_name, "http://10.0.0.1/cnc/ipxe",
        "iPXE should get HTTP URL for boot script (from configured http_server)"
    );
    // For HTTP boot, next_server can be unspecified or set to HTTP server
    // We don't strictly validate it here as it's not used for HTTP

    // Step 4: HTTP - Fetch iPXE script
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    // VALIDATION: HTTP response - Verify status and content
    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP response should be 200 OK"
    );
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/plain"),
        "HTTP response should have content-type: text/plain"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.starts_with("#!ipxe"),
        "iPXE script should start with #!ipxe shebang"
    );
    assert!(
        ipxe_script.contains("chain"),
        "iPXE script should contain 'chain' command"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}

/// Test PXE boot flow for x64 UEFI architecture
/// Expected flow: DHCP (firmware) → TFTP (ipxe.efi) → DHCP (iPXE) → HTTP (/cnc/ipxe)
#[tokio::test]
async fn test_pxe_boot_x64_uefi() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1: First DHCP exchange (UEFI firmware requesting bootloader)
    let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x57]; // Different MAC
    let dhcp_port = handle.dhcp_port;
    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::X64Uefi, dhcp_port)?;
            let offered_ip = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, Ipv4Addr::LOCALHOST)?;
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

    // VALIDATION: First-stage DHCP - Verify boot options for UEFI firmware
    assert_eq!(
        boot_options.next_server,
        Ipv4Addr::new(10, 0, 0, 1),
        "Next-server should be 10.0.0.1 (configured TFTP server)"
    );
    assert_eq!(
        boot_options.bootfile_name, "ipxe.efi",
        "Bootfile should be ipxe.efi for UEFI (iPXE EFI bootloader)"
    );

    // Step 2: TFTP - Fetch the iPXE bootloader
    let tftp_port = handle.tftp_port;
    let bootfile_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use common::tftp_client::TftpClient;
        use std::net::SocketAddr;

        let server = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tftp_port);
        let client = TftpClient::new(server)?;
        let data = client.download("ipxe.efi")?;
        Ok(data)
    })
    .await??;

    // VALIDATION: TFTP download - Verify we got the UEFI bootloader content
    assert!(
        !bootfile_content.is_empty(),
        "TFTP download should return non-empty bootfile"
    );
    assert_eq!(
        bootfile_content.len(),
        182,
        "TFTP bootfile should be 182 bytes (our mock ipxe.efi)"
    );
    assert!(
        bootfile_content.starts_with(b"Mock iPXE EFI"),
        "TFTP bootfile should start with 'Mock iPXE EFI' (our test fixture)"
    );

    // Step 3: Second DHCP exchange (iPXE requesting boot script URL)
    let dhcp_port = handle.dhcp_port;
    let (leased_ip2, ipxe_boot_options) = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut dhcp_client = DhcpClient::new(mac, Architecture::X64Uefi, dhcp_port)?;
        let offered_ip = dhcp_client.discover()?;
        let (leased_ip, boot_options) =
            dhcp_client.request_as_ipxe(offered_ip, Ipv4Addr::LOCALHOST)?;
        Ok((leased_ip, boot_options))
    })
    .await??;

    // VALIDATION: Second-stage DHCP - Verify iPXE gets HTTP URL
    assert_eq!(
        leased_ip2, leased_ip,
        "iPXE should receive same IP address as firmware"
    );
    // HTTP URL will use configured http_server from DHCP config (10.0.0.1)
    assert_eq!(
        ipxe_boot_options.bootfile_name, "http://10.0.0.1/cnc/ipxe",
        "iPXE should get HTTP URL for boot script (from configured http_server)"
    );

    // Step 4: HTTP - Fetch iPXE script (iPXE bootloader would do this)
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    // VALIDATION: HTTP response - Verify status and content
    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP response should be 200 OK"
    );
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/plain"),
        "HTTP response should have content-type: text/plain"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.starts_with("#!ipxe"),
        "iPXE script should start with #!ipxe shebang"
    );
    assert!(
        ipxe_script.contains("chain"),
        "iPXE script should contain 'chain' command"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}

/// Test PXE boot flow for ARM64 UEFI architecture
/// Expected flow: DHCP (firmware) → TFTP (ipxe.efi) → DHCP (iPXE) → HTTP (/cnc/ipxe)
#[tokio::test]
async fn test_pxe_boot_arm64_uefi() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1: First DHCP exchange (ARM64 UEFI firmware requesting bootloader)
    let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x58]; // Different MAC
    let dhcp_port = handle.dhcp_port;
    let (offered_ip, leased_ip, boot_options) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mut dhcp_client = DhcpClient::new(mac, Architecture::Arm64Uefi, dhcp_port)?;
            let offered_ip = dhcp_client.discover()?;
            let (leased_ip, boot_options) = dhcp_client.request(offered_ip, Ipv4Addr::LOCALHOST)?;
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

    // VALIDATION: First-stage DHCP - Verify boot options for ARM64 UEFI firmware
    assert_eq!(
        boot_options.next_server,
        Ipv4Addr::new(10, 0, 0, 1),
        "Next-server should be 10.0.0.1 (configured TFTP server)"
    );
    assert_eq!(
        boot_options.bootfile_name, "ipxe.efi",
        "Bootfile should be ipxe.efi for ARM64 UEFI (iPXE EFI bootloader)"
    );

    // Step 2: TFTP - Fetch the iPXE bootloader
    let tftp_port = handle.tftp_port;
    let bootfile_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use common::tftp_client::TftpClient;
        use std::net::SocketAddr;

        let server = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tftp_port);
        let client = TftpClient::new(server)?;
        let data = client.download("ipxe.efi")?;
        Ok(data)
    })
    .await??;

    // VALIDATION: TFTP download - Verify we got the ARM64 UEFI bootloader content
    assert!(
        !bootfile_content.is_empty(),
        "TFTP download should return non-empty bootfile"
    );
    assert_eq!(
        bootfile_content.len(),
        182,
        "TFTP bootfile should be 182 bytes (our mock ipxe.efi)"
    );
    assert!(
        bootfile_content.starts_with(b"Mock iPXE EFI"),
        "TFTP bootfile should start with 'Mock iPXE EFI' (our test fixture)"
    );

    // Step 3: Second DHCP exchange (iPXE requesting boot script URL)
    let dhcp_port = handle.dhcp_port;
    let (leased_ip2, ipxe_boot_options) = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut dhcp_client = DhcpClient::new(mac, Architecture::Arm64Uefi, dhcp_port)?;
        let offered_ip = dhcp_client.discover()?;
        let (leased_ip, boot_options) =
            dhcp_client.request_as_ipxe(offered_ip, Ipv4Addr::LOCALHOST)?;
        Ok((leased_ip, boot_options))
    })
    .await??;

    // VALIDATION: Second-stage DHCP - Verify iPXE gets HTTP URL
    assert_eq!(
        leased_ip2, leased_ip,
        "iPXE should receive same IP address as firmware"
    );
    // HTTP URL will use configured http_server from DHCP config (10.0.0.1)
    assert_eq!(
        ipxe_boot_options.bootfile_name, "http://10.0.0.1/cnc/ipxe",
        "iPXE should get HTTP URL for boot script (from configured http_server)"
    );

    // Step 4: HTTP - Fetch iPXE script (iPXE bootloader would do this)
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    // VALIDATION: HTTP response - Verify status and content
    assert_eq!(
        response.status().as_u16(),
        200,
        "HTTP response should be 200 OK"
    );
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/plain"),
        "HTTP response should have content-type: text/plain"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.starts_with("#!ipxe"),
        "iPXE script should start with #!ipxe shebang"
    );
    assert!(
        ipxe_script.contains("chain"),
        "iPXE script should contain 'chain' command"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}
