use anyhow::{Result, anyhow};
use dhcproto::{
    Decodable, Decoder, Encodable, Encoder,
    v4::{self, DhcpOption, Flags, Message, MessageType, OptionCode},
};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use crate::ConnectionConfig;
use crate::output::Output;
use crate::server::ServerState;

/// Discover an IP address for a specific NIC
pub fn discover(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    nic_index: usize,
    output: &Output,
) -> Result<()> {
    if nic_index >= state.mac_addresses.len() {
        return Err(anyhow!("Invalid NIC index: {}", nic_index));
    }

    output.step(&format!("DHCP DISCOVER (NIC {})", nic_index));
    output.detail("MAC", &state.mac_string(Some(nic_index)));
    output.detail("Architecture", &state.architecture);

    let socket = create_socket(conn.dhcp_port)?;
    let xid = generate_xid();
    state.xid = Some(xid);

    let arch = state.architecture()?;

    let mut msg = Message::default();
    msg.set_xid(xid)
        .set_flags(Flags::default().set_broadcast())
        .set_chaddr(&state.mac_addresses[nic_index]);

    msg.opts_mut()
        .insert(DhcpOption::MessageType(MessageType::Discover));
    msg.opts_mut().insert(DhcpOption::ClientSystemArchitecture(
        v4::Architecture::from(arch.dhcp_option_93()),
    ));
    msg.opts_mut().insert(DhcpOption::ParameterRequestList(vec![
        OptionCode::SubnetMask,
        OptionCode::Router,
        OptionCode::DomainNameServer,
        OptionCode::TFTPServerName,
        OptionCode::BootfileName,
    ]));

    let mut buf = Vec::new();
    let mut encoder = Encoder::new(&mut buf);
    msg.encode(&mut encoder)?;

    output.info(&format!("Sending DISCOVER (xid: {:08x})", xid));
    socket.send(&buf)?;

    let mut recv_buf = vec![0u8; 1500];
    let len = socket.recv(&mut recv_buf)?;
    recv_buf.truncate(len);

    let mut decoder = Decoder::new(&recv_buf);
    let offer = Message::decode(&mut decoder)?;

    if offer.xid() != xid {
        return Err(anyhow!("Transaction ID mismatch"));
    }

    let msg_type = extract_message_type(&offer)?;
    if msg_type != MessageType::Offer {
        return Err(anyhow!("Expected OFFER, got {:?}", msg_type));
    }

    let offered_ip = offer.yiaddr();
    let server_ip = offer.siaddr();

    output.info("Received OFFER");
    output.detail("Offered IP", &offered_ip.to_string());
    output.detail("Server IP", &server_ip.to_string());

    // Store IP for this NIC
    state.allocated_ips[nic_index] = Some(offered_ip);

    // Also store in legacy fields for backward compatibility (use first NIC)
    if nic_index == 0 {
        state.allocated_ip = Some(offered_ip);
        state.tftp_server = Some(server_ip);
    }

    output.success(&format!(
        "DHCP DISCOVER complete for NIC {}: offered {}",
        nic_index, offered_ip
    ));

    Ok(())
}

/// Request an IP address for a specific NIC (firmware mode)
pub fn request(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    nic_index: usize,
    output: &Output,
) -> Result<()> {
    request_internal(conn, state, nic_index, output, false)
}

/// Request an IP address as iPXE (for boot script - always uses NIC 0)
pub fn request_as_ipxe(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
) -> Result<()> {
    request_internal(conn, state, 0, output, true)
}

/// Perform DHCP discovery and request for all NICs, trying one at a time
/// with a 10-second timeout per interface
pub fn discover_all_nics(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
) -> Result<()> {
    output.step(&format!(
        "DHCP Discovery for {} NICs (sequential with 10s timeout per NIC)",
        state.mac_addresses.len()
    ));

    let mut last_error: Option<anyhow::Error> = None;

    for nic_index in 0..state.mac_addresses.len() {
        output.info(&format!(
            "Trying NIC {} (MAC {})",
            nic_index,
            state.mac_string(Some(nic_index))
        ));

        // Try this NIC with a timeout
        match try_nic_with_timeout(conn, state, nic_index, output) {
            Ok(()) => {
                state.current_nic_index = nic_index;
                output.success(&format!("Successfully obtained lease on NIC {}", nic_index));
                return Ok(());
            }
            Err(e) => {
                output.info(&format!(
                    "Failed to obtain lease on NIC {}: {}",
                    nic_index, e
                ));
                last_error = Some(e);

                // Continue to next NIC if available
                if nic_index + 1 < state.mac_addresses.len() {
                    output.info("Trying next interface...");
                }
            }
        }
    }

    // If we get here, all NICs failed
    Err(last_error.unwrap_or_else(|| anyhow!("All NICs failed to obtain DHCP lease")))
}

/// Try to obtain a DHCP lease on a specific NIC with a 10-second timeout
fn try_nic_with_timeout(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    nic_index: usize,
    output: &Output,
) -> Result<()> {
    use std::time::Instant;

    let start = Instant::now();
    let timeout = Duration::from_secs(10);

    // Try discover - if it times out or fails, return error
    discover(conn, state, nic_index, output)?;

    // Check if we've exceeded our 10-second budget
    if start.elapsed() >= timeout {
        return Err(anyhow!("Timeout exceeded during discover phase"));
    }

    // Try request - if it times out or fails, return error
    request(conn, state, nic_index, output)?;

    // Check if we've exceeded our 10-second budget
    if start.elapsed() >= timeout {
        return Err(anyhow!("Timeout exceeded during request phase"));
    }

    Ok(())
}

fn request_internal(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    nic_index: usize,
    output: &Output,
    is_ipxe: bool,
) -> Result<()> {
    if nic_index >= state.mac_addresses.len() {
        return Err(anyhow!("Invalid NIC index: {}", nic_index));
    }

    let mode = if is_ipxe { "iPXE" } else { "Firmware" };
    output.step(&format!("DHCP REQUEST ({}, NIC {})", mode, nic_index));

    let requested_ip = state
        .allocated_ips
        .get(nic_index)
        .and_then(|ip| *ip)
        .ok_or_else(|| {
            anyhow!(
                "No IP allocated for NIC {}. Run dhcp-discover first.",
                nic_index
            )
        })?;
    let server_ip = state.tftp_server.unwrap_or(conn.host);

    output.detail("Requesting IP", &requested_ip.to_string());
    output.detail("Server", &server_ip.to_string());

    let socket = create_socket(conn.dhcp_port)?;
    let xid = state.xid.unwrap_or_else(generate_xid);
    let arch = state.architecture()?;

    let mut msg = Message::default();
    msg.set_xid(xid)
        .set_flags(Flags::default().set_broadcast())
        .set_chaddr(&state.mac_addresses[nic_index]);

    msg.opts_mut()
        .insert(DhcpOption::MessageType(MessageType::Request));
    msg.opts_mut()
        .insert(DhcpOption::RequestedIpAddress(requested_ip));
    msg.opts_mut()
        .insert(DhcpOption::ServerIdentifier(server_ip));
    msg.opts_mut().insert(DhcpOption::ClientSystemArchitecture(
        v4::Architecture::from(arch.dhcp_option_93()),
    ));

    if is_ipxe {
        msg.opts_mut()
            .insert(DhcpOption::UserClass(b"iPXE".to_vec()));
        output.info("Including iPXE User-Class option");
    }

    let mut buf = Vec::new();
    let mut encoder = Encoder::new(&mut buf);
    msg.encode(&mut encoder)?;

    output.info(&format!("Sending REQUEST (xid: {:08x})", xid));
    socket.send(&buf)?;

    let mut recv_buf = vec![0u8; 1500];
    let len = socket.recv(&mut recv_buf)?;
    recv_buf.truncate(len);

    let mut decoder = Decoder::new(&recv_buf);
    let ack = Message::decode(&mut decoder)?;

    if ack.xid() != xid {
        return Err(anyhow!("Transaction ID mismatch"));
    }

    let msg_type = extract_message_type(&ack)?;
    if msg_type != MessageType::Ack {
        return Err(anyhow!("Expected ACK, got {:?}", msg_type));
    }

    let leased_ip = ack.yiaddr();
    let next_server = ack.siaddr();

    let bootfile = ack
        .opts()
        .get(OptionCode::BootfileName)
        .and_then(|opt| {
            if let DhcpOption::BootfileName(name) = opt {
                Some(String::from_utf8_lossy(name).to_string())
            } else {
                None
            }
        });

    output.info("Received ACK");
    output.detail("Leased IP", &leased_ip.to_string());
    output.detail("Next Server", &next_server.to_string());
    output.detail("Bootfile", bootfile.as_ref().unwrap_or(&"None".to_owned()));

    // Store IP for this NIC
    state.allocated_ips[nic_index] = Some(leased_ip);

    // Also store in legacy fields for backward compatibility (use first NIC)
    if nic_index == 0 {
        state.allocated_ip = Some(leased_ip);
        state.tftp_server = Some(next_server);
        state.bootfile = bootfile.clone();

        if let Some(file) = &bootfile {
            if is_ipxe && file.starts_with("http") {
                state.boot_script_url = Some(file.clone());
            }
        }
    }

    output.success(&format!(
        "DHCP REQUEST complete for NIC {}: {} -> {}",
        nic_index, leased_ip, bootfile.unwrap_or("None".to_string())
    ));

    Ok(())
}

fn create_socket(server_port: u16) -> Result<UdpSocket> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;

    let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), server_port);
    socket.connect(server_addr)?;

    Ok(socket)
}

fn generate_xid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32 ^ d.subsec_nanos())
        .unwrap_or(12345)
}

fn extract_message_type(msg: &Message) -> Result<MessageType> {
    msg.opts()
        .get(OptionCode::MessageType)
        .and_then(|opt| {
            if let DhcpOption::MessageType(mt) = opt {
                Some(*mt)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("No message type in DHCP response"))
}
