use anyhow::{Result, anyhow};

use crate::ConnectionConfig;
use crate::agent;
use crate::config::ResolvedServer;
use crate::dhcp;
use crate::http::{HttpClient, parse_ipxe_script};
use crate::output::Output;
use crate::server::ServerState;
use crate::tftp;

pub async fn full_boot(
    conn: &ConnectionConfig,
    server_config: &ResolvedServer,
    output: &Output,
) -> Result<()> {
    output.step(&format!(
        "Starting full boot sequence for '{}'",
        server_config.name
    ));
    output.detail(
        "MAC",
        &format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            server_config.mac[0],
            server_config.mac[1],
            server_config.mac[2],
            server_config.mac[3],
            server_config.mac[4],
            server_config.mac[5]
        ),
    );
    output.detail("UUID", &server_config.uuid);
    output.detail("Architecture", server_config.architecture.as_str());

    let mut state = ServerState::new(&server_config.name, server_config);

    println!();
    println!("=== Phase 1: Initial PXE Boot (Firmware) ===");
    println!();

    dhcp::discover(conn, &mut state, output)?;
    dhcp::request(conn, &mut state, output)?;
    tftp::download(conn, &mut state, output)?;

    println!();
    println!("=== Phase 2: iPXE Second-Stage Boot ===");
    println!();

    ipxe_boot(conn, &mut state, output).await?;

    println!();
    println!("=== Phase 3: Agent Discovery ===");
    println!();

    agent::run(conn, &state, output).await?;

    println!();
    println!("=== Phase 4: Post-Discovery Boot (Verify Local Boot) ===");
    println!();

    state.clear_state();

    dhcp::discover(conn, &mut state, output)?;
    dhcp::request(conn, &mut state, output)?;

    let bootfile = state
        .bootfile
        .as_ref()
        .ok_or_else(|| anyhow!("No bootfile after post-discovery DHCP"))?;

    if bootfile.starts_with("http") {
        tftp::download(conn, &mut state, output)?;
    }

    let http = HttpClient::new(conn);

    output.step("iPXE Boot Script (Post-Discovery)");
    dhcp::request_as_ipxe(conn, &mut state, output)?;

    let script_url = state
        .boot_script_url
        .as_ref()
        .ok_or_else(|| anyhow!("No boot script URL from iPXE DHCP"))?;

    if script_url.contains("?uuid=") {
        let script = http.get_ipxe_script(Some(&state.uuid), output).await?;
        let parsed = parse_ipxe_script(&script);

        if parsed.is_local_boot {
            output.success("Verified: Server boots to local disk after discovery");
        } else if parsed.kernel_url.is_some() {
            output.error("Server still wants to netboot (expected local disk boot)");
            return Err(anyhow!("Expected local disk boot, but got netboot script"));
        } else if parsed.chain_url.is_some() {
            output.info("Chain URL returned, following...");
        }
    } else {
        let script = http.get_ipxe_script(None, output).await?;
        let parsed = parse_ipxe_script(&script);

        if let Some(chain_url) = parsed.chain_url
            && (chain_url.contains("{uuid}") || chain_url.contains("?uuid="))
        {
            let script = http.get_ipxe_script(Some(&state.uuid), output).await?;
            let parsed = parse_ipxe_script(&script);

            if parsed.is_local_boot {
                output.success("Verified: Server boots to local disk after discovery");
            } else {
                output.error("Server still wants to netboot after discovery");
            }
        }
    }

    println!();
    output.success(&format!(
        "Full boot sequence complete for '{}'",
        server_config.name
    ));

    state.save()?;

    Ok(())
}

pub async fn ipxe_boot(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
) -> Result<()> {
    output.step("iPXE Second-Stage Boot");

    dhcp::request_as_ipxe(conn, state, output)?;

    let http = HttpClient::new(conn);

    output.info("Fetching iPXE script (without UUID)...");
    let script1 = http.get_ipxe_script(None, output).await?;
    let parsed1 = parse_ipxe_script(&script1);

    if let Some(chain_url) = &parsed1.chain_url {
        output.info(&format!("Chain URL: {}", chain_url));
    }

    output.info("Fetching iPXE script (with UUID)...");
    let script2 = http.get_ipxe_script(Some(&state.uuid), output).await?;
    let parsed2 = parse_ipxe_script(&script2);

    if parsed2.is_local_boot {
        output.success("Boot target: Local disk");
        return Ok(());
    }

    if let Some(kernel_url) = &parsed2.kernel_url {
        output.detail("Kernel URL", kernel_url);

        if let Some(initrd_url) = &parsed2.initrd_url {
            output.detail("Initrd URL", initrd_url);
        }

        if let Some(cmdline) = &parsed2.cmdline {
            output.detail("Cmdline", cmdline);
        }

        if kernel_url.contains("/cnc/agent-images/") || kernel_url.contains("vmlinuz") {
            output.info("Boot target: Agent image (discovery)");

            output.info("Verifying agent image is accessible...");
            let _kernel = http.get_agent_image("vmlinuz", output).await?;
            let _initrd = http.get_agent_image("initramfs.img", output).await?;

            output.success("Agent images verified");
        } else {
            output.info("Boot target: OS installation");
        }
    }

    output.success("iPXE boot complete");

    Ok(())
}
