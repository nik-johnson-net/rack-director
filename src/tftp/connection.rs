use log::debug;
use std::{fmt::Display, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::UdpSocket, time::timeout};

use crate::tftp::{
    Handler,
    packet::Packet,
    state::{ControlFlow, State},
};

#[derive(Debug)]
pub enum Error {
    ConnectionClosed,
    Send(tokio::io::Error),
    Parse(String),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ConnectionClosed => write!(f, "Connection closed"),
            Error::Send(e) => write!(f, "Failed to send packet: {e}"),
            Error::Parse(msg) => write!(f, "Failed to parse packet: {msg}"),
        }
    }
}

impl From<tokio::io::Error> for Error {
    fn from(err: tokio::io::Error) -> Self {
        Error::Send(err)
    }
}

// Connection is a TFTP connection that handles a single transfer session.
// It binds to a new UDP port and manages the state of the transfer.
pub struct Connection<H: Handler> {
    addr: SocketAddr,
    socket: UdpSocket,
    state: State<H>,
}

impl<H: Handler + 'static> Connection<H> {
    // TFTP handles each connection with a separate port. Accept will bind a new UDP port
    // and create a new State for the connection.
    pub async fn accept(handler: Arc<H>, addr: SocketAddr, packet: Packet) -> Result<(), Error> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(addr).await?;
        debug!("Accepted connection from {addr}");

        let mut connection = Self {
            addr,
            socket,
            state: State::new(addr, handler),
        };

        // Handle the initial packet
        match connection.handle(packet).await {
            Ok(_) => {
                // Continue processing packets
            }
            Err(Error::ConnectionClosed) => {
                debug!("Connection closed for {}", connection.addr);
                return Ok(());
            }
            Err(e) => {
                debug!("Error handling packet: {e}");
                return Err(e);
            }
        }

        let mut buf = [0; 512]; // TFTP packets can be up to 512 bytes
        // Start the main loop to handle incoming packets
        loop {
            match timeout(Duration::from_millis(100), connection.socket.recv(&mut buf)).await {
                // Received a packet
                Ok(Ok(size)) => {
                    let packet =
                        Packet::parse(&buf[..size]).map_err(|e| Error::Parse(e.to_string()))?;

                    match connection.handle(packet).await {
                        Ok(_) => {
                            // Continue processing packets
                        }
                        Err(Error::ConnectionClosed) => {
                            debug!("Connection closed for {}", connection.addr);
                            return Ok(());
                        }
                        Err(e) => {
                            debug!("Error handling packet: {e}");
                            return Err(e);
                        }
                    }
                }
                // Socket closed or had an error
                Ok(Err(e)) => {
                    debug!("Error receiving packet: {e}");
                    return Err(Error::Send(e));
                }
                // Timeout occurred
                Err(_) => {
                    match connection.timeout().await {
                        Ok(_) => {
                            // Continue processing packets
                        }
                        Err(Error::ConnectionClosed) => {
                            debug!("Connection closed for {}", connection.addr);
                            return Ok(());
                        }
                        Err(e) => {
                            debug!("Error handling timeout: {e}");
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    async fn handle(&mut self, packet: Packet) -> std::result::Result<(), Error> {
        let control_flow = self.state.handle(packet).await;
        match control_flow {
            ControlFlow::Continue(packet) => {
                // Send the response packet back to the client
                self.socket.send(&packet.to_bytes()).await?;
            }
            ControlFlow::Closed(packet_opt) => {
                if let Some(packet) = packet_opt {
                    // Send the final packet before closing
                    self.socket.send(&packet.to_bytes()).await?;
                }
                // Close the connection
                return Err(Error::ConnectionClosed);
            }
        }

        Ok(())
    }

    async fn timeout(&mut self) -> std::result::Result<(), Error> {
        // Handle timeout logic, e.g., retransmitting packets or closing the connection
        debug!("Handling timeout for connection {}", self.addr);
        match self.state.handle_timeout().await {
            ControlFlow::Continue(packet) => {
                self.socket.send(&packet.to_bytes()).await?;
            }
            ControlFlow::Closed(packet_opt) => {
                if let Some(packet) = packet_opt {
                    self.socket.send(&packet.to_bytes()).await?;
                }
                // Close the connection
                return Err(Error::ConnectionClosed);
            }
        }
        Ok(())
    }
}
