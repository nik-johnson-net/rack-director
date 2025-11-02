mod common;

use anyhow::Result;
use common::dhcp_client::{Architecture, DhcpClient};
use std::net::Ipv4Addr;

/// Test PXE boot flow for x86 BIOS architecture
/// Expected flow: DHCP DISCOVER → DHCP OFFER → TFTP (undionly.kpxe) → HTTP (/cnc/ipxe)
#[tokio::test]
async fn test_pxe_boot_x86_bios() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1 & 2: DHCP DISCOVER/OFFER/REQUEST/ACK with BIOS architecture
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

    // Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");

    // Verify boot options for BIOS
    assert!(
        !boot_options.next_server.is_unspecified(),
        "Should have next-server address"
    );
    assert_eq!(
        boot_options.bootfile_name, "undionly.kpxe",
        "Bootfile should be undionly.kpxe for BIOS"
    );

    // Extract TFTP server address for next step (unused in test, but verified above)
    let _tftp_server_ip = boot_options.next_server;

    // Step 3: TFTP - Fetch the boot file
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

    // Verify we got content
    assert!(!bootfile_content.is_empty(), "TFTP should return bootfile");

    // Step 4: HTTP - Fetch iPXE script
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    assert!(
        response.status().is_success(),
        "HTTP request should succeed"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.contains("#!ipxe"),
        "Response should be iPXE script"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}

/// Test PXE boot flow for x64 UEFI architecture
/// Expected flow: DHCP DISCOVER → DHCP OFFER → HTTP (/cnc/ipxe) directly
#[tokio::test]
async fn test_pxe_boot_x64_uefi() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1 & 2: DHCP DISCOVER/OFFER/REQUEST/ACK with x64 UEFI architecture
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

    // Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");

    // Verify boot options for UEFI - should point to HTTP
    assert!(
        !boot_options.next_server.is_unspecified(),
        "Should have next-server address"
    );

    // For UEFI, bootfile should be HTTP URL
    assert!(
        boot_options.bootfile_name.starts_with("http://"),
        "Bootfile should be HTTP URL for UEFI, got: {}",
        boot_options.bootfile_name
    );
    assert!(
        boot_options.bootfile_name.contains("/cnc/ipxe"),
        "Bootfile should point to iPXE endpoint"
    );

    // Step 3: HTTP - Fetch iPXE script directly (no TFTP step for UEFI)
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    assert!(
        response.status().is_success(),
        "HTTP request should succeed"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.contains("#!ipxe"),
        "Response should be iPXE script"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}

/// Test PXE boot flow for ARM64 UEFI architecture
/// Expected flow: DHCP DISCOVER → DHCP OFFER → HTTP (/cnc/ipxe) directly
#[tokio::test]
async fn test_pxe_boot_arm64_uefi() -> Result<()> {
    // Start rack-director with all services
    let handle = common::start_rack_director().await?;

    // Step 1 & 2: DHCP DISCOVER/OFFER/REQUEST/ACK with ARM64 UEFI architecture
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

    // Verify we got an IP address
    assert_eq!(offered_ip, leased_ip, "Leased IP should match offered IP");

    // Verify boot options for ARM64 UEFI - should point to HTTP
    assert!(
        !boot_options.next_server.is_unspecified(),
        "Should have next-server address"
    );

    // For UEFI, bootfile should be HTTP URL
    assert!(
        boot_options.bootfile_name.starts_with("http://"),
        "Bootfile should be HTTP URL for UEFI, got: {}",
        boot_options.bootfile_name
    );
    assert!(
        boot_options.bootfile_name.contains("/cnc/ipxe"),
        "Bootfile should point to iPXE endpoint"
    );

    // Step 3: HTTP - Fetch iPXE script directly (no TFTP step for UEFI)
    let http_client = reqwest::Client::new();
    let ipxe_url = format!("http://127.0.0.1:{}/cnc/ipxe", handle.http_port);
    let response = http_client.get(&ipxe_url).send().await?;

    assert!(
        response.status().is_success(),
        "HTTP request should succeed"
    );

    let ipxe_script = response.text().await?;
    assert!(
        ipxe_script.contains("#!ipxe"),
        "Response should be iPXE script"
    );

    // Tests complete - handle will be dropped and services will stop
    drop(handle);

    Ok(())
}
