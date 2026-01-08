use anyhow::{Result, anyhow};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::ConnectionConfig;
use crate::output::Output;
use crate::server::ServerState;

pub fn download(conn: &ConnectionConfig, state: &mut ServerState, output: &Output) -> Result<()> {
    let bootfile = state
        .bootfile
        .as_ref()
        .ok_or_else(|| anyhow!("No bootfile set. Run dhcp-request first."))?;

    if bootfile.starts_with("http") {
        output.info("Bootfile is HTTP URL, skipping TFTP download");
        return Ok(());
    }

    let tftp_server = state.tftp_server.unwrap_or(conn.host);

    output.step("TFTP DOWNLOAD");
    output.detail("Server", &tftp_server.to_string());
    output.detail("Port", &conn.tftp_port.to_string());
    output.detail("File", bootfile);

    let server_addr = SocketAddr::new(tftp_server.into(), conn.tftp_port);
    let client = TftpClient::new(server_addr)?;

    let data = client.download(bootfile, output)?;

    output.success(&format!(
        "TFTP DOWNLOAD complete: {} bytes received",
        data.len()
    ));

    Ok(())
}

struct TftpClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl TftpClient {
    fn new(server_addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;

        Ok(Self {
            socket,
            server_addr,
        })
    }

    fn download(&self, filename: &str, output: &Output) -> Result<Vec<u8>> {
        let mut packet = vec![0x00, 0x01];
        packet.extend_from_slice(filename.as_bytes());
        packet.push(0);
        packet.extend_from_slice(b"octet");
        packet.push(0);

        output.info(&format!(
            "Sending RRQ for '{}' from '{}' to '{}'",
            filename,
            self.socket.local_addr().unwrap(),
            self.server_addr
        ));
        self.socket.send_to(&packet, self.server_addr)?;

        let mut file_data = Vec::new();
        let mut expected_block = 0u16;
        let mut buf = [0u8; 516];
        let mut first_packet = true;
        let mut packet_count = 0u32;

        loop {
            packet_count += 1;
            if packet_count > 1000 {
                return Err(anyhow!(
                    "Too many packets received (> 1000), likely stuck in loop"
                ));
            }

            let (size, from_addr) = self.socket.recv_from(&mut buf)?;

            if size < 4 {
                return Err(anyhow!("TFTP packet too small: {} bytes", size));
            }

            let opcode = u16::from_be_bytes([buf[0], buf[1]]);

            if opcode == 5 {
                let error_code = u16::from_be_bytes([buf[2], buf[3]]);
                let error_msg =
                    std::str::from_utf8(&buf[4..size.saturating_sub(1)]).unwrap_or("Unknown error");
                return Err(anyhow!("TFTP error {}: {}", error_code, error_msg));
            }

            if opcode != 3 {
                return Err(anyhow!("Expected DATA packet (opcode 3), got {}", opcode));
            }

            let recv_block = u16::from_be_bytes([buf[2], buf[3]]);

            if first_packet {
                expected_block = recv_block;
                first_packet = false;
            } else if recv_block != expected_block {
                continue;
            }

            let data_len = size - 4;
            file_data.extend_from_slice(&buf[4..size]);

            if output.is_verbose() && packet_count <= 3 {
                output.info(&format!(
                    "Received block {}, {} bytes",
                    recv_block, data_len
                ));
            }

            let ack = [0x00, 0x04, buf[2], buf[3]];
            self.socket.send_to(&ack, from_addr)?;

            if data_len < 512 {
                break;
            }

            expected_block = expected_block.wrapping_add(1);
        }

        output.info(&format!(
            "Transfer complete: {} blocks, {} bytes total",
            packet_count,
            file_data.len()
        ));

        Ok(file_data)
    }
}
