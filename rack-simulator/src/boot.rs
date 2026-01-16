use anyhow::{Result, anyhow};

use crate::ConnectionConfig;
use crate::agent;
use crate::config::ResolvedServer;
use crate::dhcp;
use crate::http::{HttpClient, parse_ipxe_script};
use crate::output::Output;
use crate::server::ServerState;
use crate::tftp;

/// Represents the action to take based on the iPXE script
enum BootAction {
    /// Boot from local disk (sanboot command)
    Sanboot,
    /// Boot the rack-agent for hardware discovery
    BootAgent,
    /// Boot an OS for installation
    BootOS,
}

/// Follows iPXE chain redirects and determines the boot action
///
/// This function fetches iPXE scripts from rack-director and automatically
/// follows chain redirects (important for the UUID redirect flow). It resolves
/// {uuid} and {netX/mac} placeholders in chain URLs and returns a BootAction
/// indicating what to do next.
///
/// # Arguments
///
/// * `http` - The HTTP client for making requests
/// * `state` - The current server state containing UUID and other info
/// * `output` - Output handler for logging
///
/// # Returns
///
/// Returns a `BootAction` indicating whether to boot locally, run the agent, or install an OS.
async fn follow_ipxe_chains(
    http: &HttpClient,
    state: &ServerState,
    output: &Output,
) -> Result<BootAction> {
    const MAX_CHAIN_DEPTH: u32 = 10;
    let mut chain_depth = 0;
    let mut uuid_param: Option<&str> = None;
    let mut mac_param: Option<String> = None;

    loop {
        if chain_depth >= MAX_CHAIN_DEPTH {
            return Err(anyhow!(
                "Max chain depth ({}) exceeded - possible infinite loop",
                MAX_CHAIN_DEPTH
            ));
        }

        output.info(&format!(
            "Fetching iPXE script (chain depth {})...",
            chain_depth
        ));

        let script = http
            .get_ipxe_script(uuid_param, mac_param.as_deref(), output)
            .await?;
        let parsed = parse_ipxe_script(&script);

        // Check for chain command
        if let Some(chain_url) = parsed.chain_url {
            chain_depth += 1;

            // Resolve {uuid} and {netX/mac} placeholders or follow url parameters
            let has_uuid = chain_url.contains("{uuid}") || chain_url.contains("?uuid=");
            let has_mac = chain_url.contains("{netX/mac}") || chain_url.contains("?mac=");

            if has_uuid || has_mac {
                if has_uuid {
                    output.info(&format!(
                        "Following chain to URL with UUID (depth {})...",
                        chain_depth
                    ));
                    uuid_param = Some(&state.uuid);
                }
                if has_mac {
                    output.info(&format!(
                        "Following chain to URL with MAC (depth {})...",
                        chain_depth
                    ));
                    // Use the primary MAC address (first in the list)
                    if !state.mac_addresses.is_empty() {
                        mac_param = Some(crate::server::format_mac(&state.mac_addresses[0]));
                    }
                }
                continue;
            } else {
                return Err(anyhow!("Unexpected chain URL format: {}", chain_url));
            }
        }

        // Check for local boot
        if parsed.is_local_boot {
            return Ok(BootAction::Sanboot);
        }

        // Check for kernel boot
        if let Some(kernel_url) = parsed.kernel_url {
            // Verify images are accessible
            if kernel_url.contains("/cnc/agent-images/") {
                output.info("Verifying agent images are accessible...");
                let _kernel = http.get_agent_image("vmlinuz", output).await?;
                let _initrd = http.get_agent_image("initramfs.img", output).await?;
                output.success("Agent images verified");

                return Ok(BootAction::BootAgent);
            } else {
                output.info("OS installation kernel detected");
                return Ok(BootAction::BootOS);
            }
        }

        return Err(anyhow!("Unknown iPXE script format"));
    }
}

pub async fn full_boot(
    conn: &ConnectionConfig,
    server_config: &ResolvedServer,
    output: &Output,
) -> Result<()> {
    output.step(&format!(
        "Starting dynamic boot sequence for '{}'",
        server_config.name
    ));
    // Display all MACs
    if server_config.macs.len() == 1 {
        output.detail("MAC", &crate::server::format_mac(&server_config.macs[0]));
    } else {
        for (idx, mac) in server_config.macs.iter().enumerate() {
            output.detail(
                &format!("MAC (eth{})", idx),
                &crate::server::format_mac(mac),
            );
        }
    }
    output.detail("UUID", &server_config.uuid);
    output.detail("Architecture", server_config.architecture.as_str());

    let mut state = ServerState::new(&server_config.name, server_config);
    let http = HttpClient::new(conn);

    let mut sanboot_count = 0;
    let mut reboot_count = 0;
    const MAX_REBOOTS: u32 = 10;

    loop {
        if reboot_count >= MAX_REBOOTS {
            return Err(anyhow!(
                "Max reboots ({}) exceeded - possible infinite loop",
                MAX_REBOOTS
            ));
        }

        println!();
        println!("=== Boot Cycle #{} ===", reboot_count + 1);
        println!();

        // Phase 1: Firmware DHCP + TFTP
        output.step("Phase 1: Firmware Boot (DHCP + TFTP)");
        output.info("Attempting to obtain DHCP lease (trying interfaces sequentially)...");
        dhcp::discover_all_nics(conn, &mut state, output)?;

        if state.bootfile.is_none() {
            output.success("No bootfile returned. Server will boot first ");
            state.save()?;

            println!();
            output.success(&format!(
                "Dynamic boot sequence complete for '{}'",
                server_config.name
            ));

            return Ok(());
        }

        output.info(&format!(
            "Using NIC {} for TFTP boot...",
            state.current_nic_index
        ));
        tftp::download(conn, &mut state, output)?;

        // Phase 2: iPXE boot script interpretation
        output.step("Phase 2: iPXE Boot Script");
        dhcp::request_as_ipxe(conn, &mut state, output)?;

        let boot_action = follow_ipxe_chains(&http, &state, output).await?;

        // Phase 3: Act on boot decision
        match boot_action {
            BootAction::Sanboot => {
                sanboot_count += 1;
                output.info(&format!(
                    "Sanboot #{} detected (local disk boot)",
                    sanboot_count
                ));

                if sanboot_count == 1 {
                    output
                        .info("First sanboot - simulating reboot to verify localboot persists...");
                    state.clear_state();
                    reboot_count += 1;
                    continue;
                } else {
                    output.success("Second sanboot - localboot verified, boot sequence complete!");
                    break;
                }
            }
            BootAction::BootAgent => {
                output.step("Phase 3: Agent Execution");
                agent::run(conn, &state, output).await?;

                output.info("Agent execution complete - simulating reboot...");
                state.clear_state();
                reboot_count += 1;
                continue;
            }
            BootAction::BootOS => {
                output.info("OS installation boot detected - stopping simulation");
                output.info("(OS installation is not simulated by rack-simulator)");
                break;
            }
        }
    }

    state.save()?;

    println!();
    output.success(&format!(
        "Dynamic boot sequence complete for '{}'",
        server_config.name
    ));

    Ok(())
}

pub async fn ipxe_boot(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
) -> Result<()> {
    output.step("iPXE Second-Stage Boot");
    output.info("Using NIC 0 (primary) for iPXE boot...");

    dhcp::request_as_ipxe(conn, state, output)?;

    let http = HttpClient::new(conn);

    // Get primary MAC address for queries
    let mac = if !state.mac_addresses.is_empty() {
        Some(crate::server::format_mac(&state.mac_addresses[0]))
    } else {
        None
    };

    output.info("Fetching iPXE script (without UUID)...");
    let script1 = http.get_ipxe_script(None, mac.as_deref(), output).await?;
    let parsed1 = parse_ipxe_script(&script1);

    if let Some(chain_url) = &parsed1.chain_url {
        output.info(&format!("Chain URL: {}", chain_url));
    }

    output.info("Fetching iPXE script (with UUID)...");
    let script2 = http
        .get_ipxe_script(Some(&state.uuid), mac.as_deref(), output)
        .await?;
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
