use std::{net::SocketAddr, sync::Arc};

use log::debug;

use crate::tftp::packet::{Error, Packet};
use anyhow::Result;

pub trait Handler {
    type Reader: Reader + Send + Sync;
    fn create_reader(&self, filename: &str) -> impl Future<Output = Result<Self::Reader>> + Send;
}

pub trait Reader {
    fn read(&mut self) -> impl Future<Output = Result<Vec<u8>>> + Send;
}

// ControlFlow is used to respond to TFTP packets, and to signal whether the connection should continue or be closed.
#[derive(Debug)]
pub enum ControlFlow {
    Continue(Packet),
    Closed(Option<Packet>),
}

// The TransferState enum represents the different states of a TFTP transfer.
enum TransferState<H: Handler> {
    Uninitialized,
    Reading {
        filename: String,
        mode: String,
        block: u16,
        reader: H::Reader,
        data: Vec<u8>,
    },
    Complete,
}

// The State struct holds the current state of a TFTP transfer, including the address of the client, the current transfer state, and a reference to the handler.
pub struct State<H: Handler> {
    addr: SocketAddr,
    state: TransferState<H>,
    handler: Arc<H>,
}

impl<H: Handler + 'static> State<H> {
    // Creates a new State instance with the given address and handler.
    pub fn new(addr: SocketAddr, handler: Arc<H>) -> Self {
        Self {
            addr,
            state: TransferState::Uninitialized,
            handler,
        }
    }

    // Handle a TFTP packet, transitioning between states as necessary.
    pub async fn handle(&mut self, packet: Packet) -> ControlFlow {
        let result = match &mut self.state {
            TransferState::Uninitialized => match packet {
                Packet::Rrq { filename, mode } => {
                    handle_read_request(self.handler.as_ref(), filename, mode).await
                }
                _ => self.error(None),
            },
            TransferState::Reading {
                filename,
                mode,
                block,
                reader,
                data,
            } => match packet {
                Packet::Ack { block: acked_block } => {
                    handle_ack(reader, mode, block, data, acked_block).await
                }
                Packet::Error { code, message } => {
                    log::info!(
                        "TFTP: Received error packet for {}: {:?} - {}",
                        self.addr,
                        code,
                        message
                    );
                    self.close()
                }
                _ => {
                    let filename2 = filename.clone();
                    self.error(Some(filename2))
                }
            },
            TransferState::Complete => self.error(None),
        };

        match result {
            Ok(response) => {
                if let Some(next_state) = response.next_state {
                    self.state = next_state;
                }
                response.response
            }
            Err(e) => {
                log::error!("TFTP: Error occured for {}: {:?}", self.addr, e);
                let packet = Packet::Error {
                    code: Error::Undefined,
                    message: String::from("internal error occured"),
                };
                ControlFlow::Closed(Some(packet))
            }
        }
    }

    pub async fn handle_timeout(&mut self) -> ControlFlow {
        debug!("TFTP: Timeout for {}", self.addr);
        match &self.state {
            TransferState::Uninitialized => {
                log::warn!("TFTP: Timeout in Uninitialized state for {}", self.addr);
                ControlFlow::Closed(None)
            }
            TransferState::Reading { data, block, .. } => ControlFlow::Continue(Packet::Data {
                block: *block,
                data: data.clone(),
            }),
            TransferState::Complete => {
                log::warn!("TFTP: Timeout in Complete state for {}", self.addr);
                ControlFlow::Closed(None)
            }
        }
    }

    // Return an IllegalOperation error response.
    fn error(&self, filename: Option<String>) -> Result<HandleResponse<H>> {
        debug!(
            "TFTP: Returning error {:?} to {} reading {}",
            Error::IllegalOperation,
            self.addr,
            filename.unwrap_or("[n/a]".to_owned()),
        );
        let packet = Packet::Error {
            code: Error::IllegalOperation,
            message: String::new(),
        };
        let response = HandleResponse {
            next_state: Some(TransferState::Complete),
            response: ControlFlow::Closed(Some(packet)),
        };
        Ok(response)
    }

    // Close the connection, transitioning to the Complete state.
    fn close(&self) -> Result<HandleResponse<H>> {
        debug!("TFTP: Closing connection for {}", self.addr);
        let response = HandleResponse {
            next_state: Some(TransferState::Complete),
            response: ControlFlow::Closed(None),
        };
        Ok(response)
    }
}

// The HandleResponse struct is used to encapsulate the response from handling a TFTP packet,
// including the next state to transition to and the control flow response.
struct HandleResponse<H: Handler> {
    next_state: Option<TransferState<H>>,
    response: ControlFlow,
}

// Handles an RRQ (Read Request) packet by initiating a read operation with the handler.
async fn handle_read_request<H: Handler>(
    handler: &H,
    filename: String,
    mode: String,
) -> Result<HandleResponse<H>> {
    let mut reader = handler.create_reader(&filename).await?;
    let data = reader.read().await?;
    let next_state = TransferState::Reading {
        filename,
        mode,
        block: 0,
        reader,
        data: data.clone(),
    };
    let reply = Packet::Data { block: 0, data };
    Ok(HandleResponse {
        next_state: Some(next_state),
        response: ControlFlow::Continue(reply),
    })
}

// Handles an ACK (Acknowledgment) packet by updating the block number and reading the next data chunk.
async fn handle_ack<H: Handler>(
    reader: &mut H::Reader,
    _mode: &str,
    block: &mut u16,
    data: &mut Vec<u8>,
    acked_block: u16,
) -> Result<HandleResponse<H>> {
    let next_block = *block + 1;
    let data = if acked_block == *block {
        // If the ACK is for the current block, and the current block is < 512 bytes, the transfer is complete.
        if data.len() < 512 {
            debug!("TFTP: Transfer complete for block {acked_block}");
            return Ok(HandleResponse {
                next_state: Some(TransferState::Complete),
                response: ControlFlow::Closed(None),
            });
        }
        // Otherwise, read the next block of data.
        *block = next_block;
        *data = reader.read().await?;
        data.clone()
    } else if acked_block == *block - 1 {
        // If the ACK is for the last block, resend the last block.
        data.clone()
    } else {
        // If the ACK is for a block that is not expected, return an error.
        return Err(anyhow::anyhow!(
            "Unexpected ACK block number: {}",
            acked_block
        ));
    };

    let reply = Packet::Data {
        block: acked_block + 1,
        data,
    };
    Ok(HandleResponse {
        next_state: None,
        response: ControlFlow::Continue(reply),
    })
}

#[cfg(test)]
mod tests {
    use crate::tftp::packet;

    use super::*;
    use std::{net::SocketAddr, str::FromStr, vec};

    struct MockHandler {
        data: Vec<u8>,
    }

    impl MockHandler {
        fn with_data(data: Vec<u8>) -> Self {
            MockHandler { data }
        }
    }

    impl Handler for MockHandler {
        type Reader = MockReader;

        async fn create_reader(&self, _filename: &str) -> Result<Self::Reader> {
            Ok(MockReader {
                data: self.data.chunks(512).map(|x| x.into()).collect(),
                next_block: 0,
            })
        }
    }

    struct MockReader {
        data: Vec<Vec<u8>>,
        next_block: u32,
    }

    impl Reader for MockReader {
        async fn read(&mut self) -> Result<Vec<u8>> {
            let block = self.next_block as usize;
            self.next_block += 1;
            Ok(self.data.get(block).cloned().unwrap())
        }
    }

    // Test a normal read connection flow.
    #[tokio::test]
    async fn test_connection() {
        // Simulate TFTP read request
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 513])),
        );
        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
            })
            .await;

        assert!(
            matches!(result, ControlFlow::Continue(packet::Packet::Data { block: 0, ref data }) if data == &vec![0; 512]),
            "Got response {result:?}"
        );

        // Simulate ACK for the first block
        let result = state.handle(Packet::Ack { block: 0 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(packet::Packet::Data { block: 1, ref data }) if data == &vec![0; 1]),
            "Got response {result:?}"
        );

        // Simulate ACK for the second block
        let result = state.handle(Packet::Ack { block: 1 }).await;
        assert!(
            matches!(result, ControlFlow::Closed(None)),
            "Got response {result:?}"
        );
    }
}
