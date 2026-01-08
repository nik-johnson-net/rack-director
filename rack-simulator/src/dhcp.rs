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

pub fn discover(conn: &ConnectionConfig, state: &mut ServerState, output: &Output) -> Result<()> {
    output.step("DHCP DISCOVER");
    output.detail("MAC", &state.mac_string());
    output.detail("Architecture", &state.architecture);

    let socket = create_socket(conn.dhcp_port)?;
    let xid = generate_xid();
    state.xid = Some(xid);

    let arch = state.architecture()?;

    let mut msg = Message::default();
    msg.set_xid(xid)
        .set_flags(Flags::default().set_broadcast())
        .set_chaddr(&state.mac_address);

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

    state.allocated_ip = Some(offered_ip);
    state.tftp_server = Some(server_ip);

    output.success(&format!("DHCP DISCOVER complete: offered {}", offered_ip));

    Ok(())
}

pub fn request(conn: &ConnectionConfig, state: &mut ServerState, output: &Output) -> Result<()> {
    request_internal(conn, state, output, false)
}

pub fn request_as_ipxe(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
) -> Result<()> {
    request_internal(conn, state, output, true)
}

fn request_internal(
    conn: &ConnectionConfig,
    state: &mut ServerState,
    output: &Output,
    is_ipxe: bool,
) -> Result<()> {
    let mode = if is_ipxe { "iPXE" } else { "Firmware" };
    output.step(&format!("DHCP REQUEST ({})", mode));

    let requested_ip = state
        .allocated_ip
        .ok_or_else(|| anyhow!("No IP allocated. Run dhcp-discover first."))?;
    let server_ip = state.tftp_server.unwrap_or(conn.host);

    output.detail("Requesting IP", &requested_ip.to_string());
    output.detail("Server", &server_ip.to_string());

    let socket = create_socket(conn.dhcp_port)?;
    let xid = state.xid.unwrap_or_else(generate_xid);
    let arch = state.architecture()?;

    let mut msg = Message::default();
    msg.set_xid(xid)
        .set_flags(Flags::default().set_broadcast())
        .set_chaddr(&state.mac_address);

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
        })
        .ok_or_else(|| anyhow!("No bootfile name in DHCP ACK"))?;

    output.info("Received ACK");
    output.detail("Leased IP", &leased_ip.to_string());
    output.detail("Next Server", &next_server.to_string());
    output.detail("Bootfile", &bootfile);

    state.allocated_ip = Some(leased_ip);
    state.tftp_server = Some(next_server);
    state.bootfile = Some(bootfile.clone());

    if is_ipxe && bootfile.starts_with("http") {
        state.boot_script_url = Some(bootfile.clone());
    }

    output.success(&format!(
        "DHCP REQUEST complete: {} -> {}",
        leased_ip, bootfile
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
