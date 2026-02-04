use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

use crate::tftp::{connection::Connection, packet::Packet};

mod connection;
mod options;
mod packet;
mod state;
pub use state::Handler;
pub use state::Reader;

pub struct StartResult {
    pub join_handle: JoinHandle<Result<()>>,
    pub port: u16,
}

pub struct Server<H: Handler> {
    address: SocketAddr,
    handler: Arc<H>,
}

impl<H: Handler + Send + Sync + 'static> Server<H> {
    pub fn new(handler: Arc<H>) -> Self {
        Self {
            address: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 69).into(),
            handler,
        }
    }

    #[allow(unused)]
    pub fn address(&mut self, addr: SocketAddr) -> &mut Self {
        self.address = addr;
        self
    }

    pub async fn serve(self) -> Result<StartResult> {
        let socket = tokio::net::UdpSocket::bind(self.address).await?;
        let port = socket.local_addr()?.port();
        let join_handle = tokio::spawn(serve(socket, self.handler));
        Ok(StartResult { join_handle, port })
    }
}

async fn serve<H: Handler + Send + Sync + 'static>(
    socket: UdpSocket,
    handler: Arc<H>,
) -> Result<()> {
    let arc_socket = Arc::new(socket);
    let mut buf: [u8; 512] = [0; 512];
    log::info!(
        "Starting TFTP server on {}",
        arc_socket.local_addr().unwrap()
    );
    loop {
        let (size, addr) = arc_socket.recv_from(&mut buf).await?;
        let packet = Packet::parse(&buf[0..size])?;
        log::info!("TFTP {:?}", packet);
        tokio::spawn(Connection::accept(handler.clone(), addr, packet));
    }
}

/// TFTP-specific file reader that reads files in chunks.
///
/// This reader wraps a tokio BufReader and provides chunk-based reading
/// suitable for TFTP block transfers.
pub struct TftpReader {
    file: BufReader<tokio::fs::File>,
    block_size: u64,
}

impl TftpReader {
    /// Open a file for TFTP reading.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path to the file
    /// * `block_size` - The size of each block to read (typically 512 bytes)
    ///
    /// # Returns
    ///
    /// Returns a new TftpReader if the file can be opened successfully.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened.
    pub async fn open(path: &Path, block_size: u64) -> Result<Self> {
        let file = tokio::fs::File::open(path).await?;
        Ok(TftpReader {
            file: BufReader::new(file),
            block_size,
        })
    }
}

impl Reader for TftpReader {
    async fn read(&mut self) -> Result<Vec<u8>> {
        let mut buffered: usize = 0;
        let mut chunk = vec![0; self.block_size as usize];

        // read() is not guaranteed to fill buffer. Keep trying until it returns n = 0 or we've filled the buffer.
        while buffered < self.block_size as usize {
            let n = self.file.read(&mut chunk[buffered..]).await?;
            if n == 0 {
                break;
            }
            buffered += n;
        }

        chunk.truncate(buffered); // Return only the bytes that were actually read
        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    //! Integration tests for TFTP port allocation per RFC 1350.
    //!
    //! RFC 1350 requires that TFTP servers use ephemeral ports (not port 69) for data transfer.
    //! Each connection should bind to a new UDP port to handle concurrent transfers.

    use super::*;
    use anyhow::Result;

    // Helper function to start a test TFTP server.
    //
    // Starts a server with the given handler bound to an ephemeral port.
    // Returns the server's listening port and join handle for cleanup.
    async fn start_test_server<H: Handler + Send + Sync + 'static>(
        handler: H,
    ) -> Result<(u16, JoinHandle<Result<()>>)> {
        let mut server = Server::new(Arc::new(handler));
        server.address(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into());
        let result = server.serve().await?;
        Ok((result.port, result.join_handle))
    }

    // Helper function to assert a port is in the ephemeral range.
    //
    // Validates that the port is >= 1024 (ephemeral range).
    fn assert_ephemeral_port(port: u16, context: &str) {
        assert!(
            port >= 1024,
            "RFC 1350 violation: {} port {} must be ephemeral (>= 1024)",
            context,
            port
        );
    }

    // Helper function to assert transfer port differs from listening port.
    //
    // Validates that the transfer uses a unique port per RFC 1350.
    fn assert_different_from_listening_port(
        transfer_port: u16,
        listening_port: u16,
        context: &str,
    ) {
        assert_ne!(
            transfer_port, listening_port,
            "RFC 1350 violation: {} port {} must differ from listening port {}",
            context, transfer_port, listening_port
        );
    }

    // Test handler that serves data from an in-memory buffer.
    struct TestHandler {
        data: Vec<u8>,
    }

    impl TestHandler {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl Handler for TestHandler {
        type Reader = TestReader;

        async fn create_reader(&self, _filename: &str, block_size: u64) -> Result<Self::Reader> {
            Ok(TestReader {
                data: self
                    .data
                    .chunks(block_size as usize)
                    .map(|x| x.to_vec())
                    .collect(),
                next_block: 0,
            })
        }

        async fn filesize(&self, _filename: &str) -> Result<u64> {
            Ok(self.data.len() as u64)
        }
    }

    // Test reader that returns data in 512-byte chunks.
    struct TestReader {
        data: Vec<Vec<u8>>,
        next_block: usize,
    }

    impl Reader for TestReader {
        async fn read(&mut self) -> Result<Vec<u8>> {
            let block = self.next_block;
            self.next_block += 1;
            self.data
                .get(block)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Block {} out of bounds", block))
        }
    }

    // Test handler that always returns errors.
    struct ErrorHandler;

    impl Handler for ErrorHandler {
        type Reader = TestReader;

        async fn create_reader(&self, _filename: &str, _block_size: u64) -> Result<Self::Reader> {
            Err(anyhow::anyhow!("File not found"))
        }

        async fn filesize(&self, _filename: &str) -> Result<u64> {
            Err(anyhow::anyhow!("File not found"))
        }
    }

    // Helper function to get the transfer port used by the server.
    //
    // Sends an RRQ to the server and extracts the source port from the first DATA packet.
    // The function returns immediately after receiving the first packet to verify port allocation.
    async fn get_transfer_port(server_port: u16, filename: &str) -> Result<u16> {
        let client_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;

        // Build RRQ packet
        let rrq = Packet::Rrq {
            filename: filename.to_string(),
            mode: "octet".to_string(),
            options: Vec::new(),
        };
        let rrq_bytes = rrq.to_bytes();

        // Send RRQ to server
        client_socket
            .send_to(&rrq_bytes, format!("127.0.0.1:{}", server_port))
            .await?;

        // Receive response with timeout
        let mut buf = [0u8; 516];
        let recv_result = tokio::time::timeout(
            tokio::time::Duration::from_millis(1000),
            client_socket.recv_from(&mut buf),
        )
        .await?;

        let (size, addr) = recv_result?;
        let transfer_port = addr.port();

        // Parse the response and validate it's a DATA packet
        let packet = Packet::parse(&buf[..size])?;
        assert!(
            matches!(packet, Packet::Data { .. }),
            "Expected DATA packet, got {:?}",
            packet
        );

        // Drop the socket immediately to abort the transfer
        // This is intentional - we only need to verify the port allocation
        drop(client_socket);

        // Return the source port of the response
        Ok(transfer_port)
    }

    /// Test that a single transfer uses an ephemeral port (not the listening port).
    ///
    /// RFC 1350 requires that each transfer uses a unique TID (transfer identifier),
    /// which is implemented as a new UDP port binding.
    #[tokio::test]
    async fn test_port_allocation_single_transfer() -> Result<()> {
        // Start server with test data
        let handler = TestHandler::new(vec![0u8; 100]);
        let (listening_port, join_handle) = start_test_server(handler).await?;

        // Perform a transfer and get the port used
        let transfer_port = get_transfer_port(listening_port, "test.txt").await?;

        // Cleanup
        join_handle.abort();

        // Assert transfer port is different from listening port
        assert_different_from_listening_port(transfer_port, listening_port, "Transfer");

        // Assert transfer port is in ephemeral range
        assert_ephemeral_port(transfer_port, "Transfer");

        Ok(())
    }

    /// Test that multiple concurrent transfers each get unique ports.
    ///
    /// RFC 1350 requires each concurrent transfer to have its own TID.
    #[tokio::test]
    async fn test_port_allocation_multiple_concurrent() -> Result<()> {
        // Start server with test data
        let handler = TestHandler::new(vec![0u8; 100]);
        let (listening_port, join_handle) = start_test_server(handler).await?;

        // Start 3 concurrent transfers
        let (port1, port2, port3) = tokio::join!(
            get_transfer_port(listening_port, "file1.txt"),
            get_transfer_port(listening_port, "file2.txt"),
            get_transfer_port(listening_port, "file3.txt"),
        );

        let port1 = port1?;
        let port2 = port2?;
        let port3 = port3?;

        // Cleanup
        join_handle.abort();

        // Assert all ports are different from listening port
        assert_different_from_listening_port(port1, listening_port, "Transfer 1");
        assert_different_from_listening_port(port2, listening_port, "Transfer 2");
        assert_different_from_listening_port(port3, listening_port, "Transfer 3");

        // Assert all transfer ports are unique
        assert_ne!(
            port1, port2,
            "RFC 1350 violation: Concurrent transfers must use unique ports"
        );
        assert_ne!(
            port1, port3,
            "RFC 1350 violation: Concurrent transfers must use unique ports"
        );
        assert_ne!(
            port2, port3,
            "RFC 1350 violation: Concurrent transfers must use unique ports"
        );

        // Assert all are in ephemeral range
        assert_ephemeral_port(port1, "Transfer 1");
        assert_ephemeral_port(port2, "Transfer 2");
        assert_ephemeral_port(port3, "Transfer 3");

        Ok(())
    }

    /// Test that the port allocation mechanism works for sequential transfers.
    ///
    /// Verifies that the server can handle multiple sequential transfers,
    /// properly allocating ephemeral ports each time.
    #[tokio::test]
    async fn test_port_allocation_reuse() -> Result<()> {
        // Start server with test data
        let handler = TestHandler::new(vec![0u8; 100]);
        let (listening_port, join_handle) = start_test_server(handler).await?;

        // Perform first transfer
        let port1 = get_transfer_port(listening_port, "test1.txt").await?;

        // Small delay to allow first transfer to clean up
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Perform second transfer
        let port2 = get_transfer_port(listening_port, "test2.txt").await?;

        // Cleanup
        join_handle.abort();

        // Both should use ephemeral ports
        assert_ephemeral_port(port1, "First transfer");
        assert_ephemeral_port(port2, "Second transfer");

        // Both should differ from listening port
        assert_different_from_listening_port(port1, listening_port, "First transfer");
        assert_different_from_listening_port(port2, listening_port, "Second transfer");

        Ok(())
    }

    /// Test that error responses also use ephemeral ports.
    ///
    /// RFC 1350 requires that even ERROR packets are sent from the transfer port,
    /// not the listening port.
    #[tokio::test]
    async fn test_port_allocation_error_response() -> Result<()> {
        // Start server with error handler
        let (listening_port, join_handle) = start_test_server(ErrorHandler).await?;

        // Try to get a nonexistent file
        let client_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;

        // Build RRQ packet
        let rrq = Packet::Rrq {
            filename: "nonexistent.txt".to_string(),
            mode: "octet".to_string(),
            options: Vec::new(),
        };
        let rrq_bytes = rrq.to_bytes();

        client_socket
            .send_to(&rrq_bytes, format!("127.0.0.1:{}", listening_port))
            .await?;

        // Receive error response with timeout (error response closes connection, no ACK needed)
        let mut buf = [0u8; 516];
        let recv_result = tokio::time::timeout(
            tokio::time::Duration::from_millis(1000),
            client_socket.recv_from(&mut buf),
        )
        .await?;

        let (size, addr) = recv_result?;
        let error_port = addr.port();

        // Parse the response and validate it's an ERROR packet
        let packet = Packet::parse(&buf[..size])?;
        assert!(
            matches!(packet, Packet::Error { .. }),
            "Expected ERROR packet, got {:?}",
            packet
        );

        // Drop socket and cleanup
        drop(client_socket);
        join_handle.abort();

        // Assert error response comes from ephemeral port
        assert_different_from_listening_port(error_port, listening_port, "ERROR response");
        assert_ephemeral_port(error_port, "ERROR response");

        Ok(())
    }
}
