use anyhow::{Result, anyhow};
use dhcproto::{
    Decodable, Decoder, Encodable, Encoder,
    v4::{self, DhcpOption, Flags, Message, MessageType, OptionCode},
};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

/// Client System Architecture types from RFC 4578
#[derive(Debug, Clone, Copy)]
pub enum Architecture {
    /// x86 BIOS (Intel x86PC)
    X86Bios = 0,
    /// x64 UEFI (EFI BC / EFI x86-64)
    X64Uefi = 7,
    /// ARM64 UEFI (EFI AArch64)
    Arm64Uefi = 11,
}

/// DHCP boot options extracted from DHCP response
#[derive(Debug, Clone)]
pub struct BootOptions {
    pub next_server: Ipv4Addr,
    pub bootfile_name: String,
}

/// DHCP client simulator for integration testing
pub struct DhcpClient {
    mac: [u8; 6],
    architecture: Architecture,
    socket: UdpSocket,
    xid: u32,
}

impl DhcpClient {
    /// Create a new DHCP client with the given MAC address and architecture
    pub fn new(mac: [u8; 6], architecture: Architecture, server_port: u16) -> Result<Self> {
        // Bind to a random port on localhost
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;

        // Connect to the DHCP server
        let server_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), server_port);
        socket.connect(server_addr)?;

        // Generate a random transaction ID
        let xid = rand::random::<u32>();

        Ok(Self {
            mac,
            architecture,
            socket,
            xid,
        })
    }

    /// Send DHCP DISCOVER and receive DHCP OFFER
    pub fn discover(&mut self) -> Result<Ipv4Addr> {
        // Build DHCP DISCOVER message
        let mut msg = Message::default();
        msg.set_xid(self.xid)
            .set_flags(Flags::default().set_broadcast())
            .set_chaddr(&self.mac);

        // Add required options
        msg.opts_mut()
            .insert(DhcpOption::MessageType(MessageType::Discover));

        // Add Client System Architecture (Option 93)
        msg.opts_mut().insert(DhcpOption::ClientSystemArchitecture(
            v4::Architecture::from(self.architecture as u16),
        ));

        // Add Parameter Request List
        msg.opts_mut().insert(DhcpOption::ParameterRequestList(vec![
            OptionCode::SubnetMask,
            OptionCode::Router,
            OptionCode::DomainNameServer,
            OptionCode::TFTPServerName,
            OptionCode::BootfileName,
        ]));

        // Encode and send
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;
        self.socket.send(&buf)?;

        // Receive DHCP OFFER
        let mut recv_buf = vec![0u8; 1500];
        let len = self.socket.recv(&mut recv_buf)?;
        recv_buf.truncate(len);

        let mut decoder = Decoder::new(&recv_buf);
        let offer = Message::decode(&mut decoder)?;

        // Verify it's an OFFER for our transaction
        if offer.xid() != self.xid {
            return Err(anyhow!("Transaction ID mismatch"));
        }

        let msg_type = offer
            .opts()
            .get(OptionCode::MessageType)
            .and_then(|opt| {
                if let DhcpOption::MessageType(mt) = opt {
                    Some(mt)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("No message type in DHCP response"))?;

        if *msg_type != MessageType::Offer {
            return Err(anyhow!("Expected OFFER, got {:?}", msg_type));
        }

        // Extract offered IP
        let offered_ip = offer.yiaddr();

        Ok(offered_ip)
    }

    /// Send DHCP REQUEST and receive DHCP ACK with boot options
    pub fn request(
        &mut self,
        requested_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
    ) -> Result<(Ipv4Addr, BootOptions)> {
        self.request_internal(requested_ip, server_ip, false)
    }

    /// Send DHCP REQUEST as iPXE (second-stage boot) and receive HTTP boot script URL
    pub fn request_as_ipxe(
        &mut self,
        requested_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
    ) -> Result<(Ipv4Addr, BootOptions)> {
        self.request_internal(requested_ip, server_ip, true)
    }

    fn request_internal(
        &mut self,
        requested_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
        is_ipxe: bool,
    ) -> Result<(Ipv4Addr, BootOptions)> {
        // Build DHCP REQUEST message
        let mut msg = Message::default();
        msg.set_xid(self.xid)
            .set_flags(Flags::default().set_broadcast())
            .set_chaddr(&self.mac);

        // Add required options
        msg.opts_mut()
            .insert(DhcpOption::MessageType(MessageType::Request));

        msg.opts_mut()
            .insert(DhcpOption::RequestedIpAddress(requested_ip));

        msg.opts_mut()
            .insert(DhcpOption::ServerIdentifier(server_ip));

        // Add Client System Architecture (Option 93)
        msg.opts_mut().insert(DhcpOption::ClientSystemArchitecture(
            v4::Architecture::from(self.architecture as u16),
        ));

        // If this is iPXE, add User-Class option (Option 77)
        if is_ipxe {
            msg.opts_mut()
                .insert(DhcpOption::UserClass(b"iPXE".to_vec()));
        }

        // Encode and send
        let mut buf = Vec::new();
        let mut encoder = Encoder::new(&mut buf);
        msg.encode(&mut encoder)?;
        self.socket.send(&buf)?;

        // Receive DHCP ACK
        let mut recv_buf = vec![0u8; 1500];
        let len = self.socket.recv(&mut recv_buf)?;
        recv_buf.truncate(len);

        let mut decoder = Decoder::new(&recv_buf);
        let ack = Message::decode(&mut decoder)?;

        // Verify it's an ACK for our transaction
        if ack.xid() != self.xid {
            return Err(anyhow!("Transaction ID mismatch"));
        }

        let msg_type = ack
            .opts()
            .get(OptionCode::MessageType)
            .and_then(|opt| {
                if let DhcpOption::MessageType(mt) = opt {
                    Some(mt)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("No message type in DHCP response"))?;

        if *msg_type != MessageType::Ack {
            return Err(anyhow!("Expected ACK, got {:?}", msg_type));
        }

        // Extract IP and boot options from ACK
        let leased_ip = ack.yiaddr();
        let boot_options = self.extract_boot_options(&ack)?;

        Ok((leased_ip, boot_options))
    }

    /// Extract boot options (next-server and bootfile name) from DHCP message
    fn extract_boot_options(&self, msg: &Message) -> Result<BootOptions> {
        // Get next-server from siaddr field (may be unspecified for HTTP boot)
        let next_server = msg.siaddr();

        // Get bootfile name from Option 67
        let bootfile_name = msg
            .opts()
            .get(OptionCode::BootfileName)
            .and_then(|opt| {
                if let DhcpOption::BootfileName(name) = opt {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("No bootfile name in DHCP response"))?;

        Ok(BootOptions {
            next_server,
            bootfile_name,
        })
    }
}

// We need rand for generating transaction IDs
use std::cell::Cell;
thread_local! {
    static RNG_SEED: Cell<u32> = Cell::new(0);
}

mod rand {
    use super::RNG_SEED;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn random<T: From<u32>>() -> T {
        RNG_SEED.with(|seed| {
            let mut s = seed.get();
            if s == 0 {
                s = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as u32;
            }
            // Simple LCG
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            seed.set(s);
            T::from(s)
        })
    }
}
