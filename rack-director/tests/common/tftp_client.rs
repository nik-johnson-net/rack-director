#![allow(dead_code)]

use anyhow::{Result, anyhow};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// Minimal TFTP client for testing purposes
/// Implements RFC 1350 TFTP Read Request (RRQ)
pub struct TftpClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl TftpClient {
    pub fn new(server_addr: SocketAddr) -> Result<Self> {
        // Bind to any available port
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;

        // Don't connect - TFTP uses different ports for data transfer
        Ok(Self {
            socket,
            server_addr,
        })
    }

    pub fn download(&self, filename: &str) -> Result<Vec<u8>> {
        // Build RRQ (Read Request) packet
        // Format: 2 bytes opcode (0x0001) + filename + 0 + mode + 0
        let mut packet = vec![0x00, 0x01]; // RRQ opcode
        packet.extend_from_slice(filename.as_bytes());
        packet.push(0); // null terminator
        packet.extend_from_slice(b"octet"); // mode
        packet.push(0); // null terminator

        // Send RRQ to server
        self.socket.send_to(&packet, self.server_addr)?;

        let mut file_data = Vec::new();
        let mut expected_block = 0u16; // Some servers start at 0, others at 1
        let mut buf = [0u8; 516]; // Max TFTP packet size (4 bytes header + 512 data)
        let mut first_packet = true;
        let mut packet_count = 0u32;

        loop {
            packet_count += 1;
            if packet_count > 100 {
                return Err(anyhow!(
                    "Too many packets received (> 100), likely stuck in loop"
                ));
            }

            // Receive DATA packet
            let (size, from_addr) = self.socket.recv_from(&mut buf)?;

            // Check opcode (should be DATA = 0x0003)
            if size < 4 {
                return Err(anyhow!("TFTP packet too small: {} bytes", size));
            }

            let opcode = u16::from_be_bytes([buf[0], buf[1]]);
            if opcode == 5 {
                // ERROR packet
                let error_code = u16::from_be_bytes([buf[2], buf[3]]);
                let error_msg = std::str::from_utf8(&buf[4..size - 1]).unwrap_or("Unknown error");
                return Err(anyhow!("TFTP error {}: {}", error_code, error_msg));
            }

            if opcode != 3 {
                return Err(anyhow!("Expected DATA packet, got opcode {}", opcode));
            }

            let recv_block = u16::from_be_bytes([buf[2], buf[3]]);

            // On the first packet, figure out if server starts at 0 or 1
            if first_packet {
                expected_block = recv_block;
                first_packet = false;
                eprintln!("TFTP: First block is {}, data_len={}", recv_block, size - 4);
            } else if recv_block != expected_block {
                eprintln!(
                    "TFTP: Block mismatch at packet {}: expected {}, got {}",
                    packet_count, expected_block, recv_block
                );
                // Server might be retransmitting - just skip duplicate blocks
                continue;
            }

            // Extract data
            let data_len = size - 4;
            eprintln!("TFTP: Received block {}, data_len={}", recv_block, data_len);
            file_data.extend_from_slice(&buf[4..size]);

            // Send ACK
            let ack = [
                0x00, 0x04, // ACK opcode
                buf[2], buf[3], // Block number
            ];
            self.socket.send_to(&ack, from_addr)?;

            // If data block is less than 512 bytes, this is the last block
            if data_len < 512 {
                break;
            }

            expected_block = expected_block.wrapping_add(1);
        }

        Ok(file_data)
    }
}
