//! TFTP transfer state machine implementation.
//!
//! This module implements RFC 1350 (TFTP Protocol) and RFC 2347 (TFTP Option Extension).
//!
//! # RFC 1350 Block Numbering
//!
//! Per RFC 1350, block numbers are consecutive and begin with one:
//! - DATA packets start at block 1 (not block 0)
//! - ACK packets acknowledge the block number of the DATA packet received
//! - Block 0 is ONLY used for ACK in response to WRQ (write requests) or OACK
//!
//! # RFC 2347 Option Negotiation
//!
//! The option negotiation flow follows this sequence:
//!
//! 1. Client sends RRQ/WRQ with optional key-value option pairs
//! 2. If server recognizes any options, it sends OACK with negotiated options
//! 3. Client sends ACK block 0 to accept the negotiated options
//! 4. Server begins data transfer with DATA block 1 (or waits for WRQ data)
//!
//! If no options are recognized, the server skips OACK and sends DATA block 1 immediately.
//!
//! ## Current Implementation Status
//!
//! The framework for option negotiation is fully implemented:
//! - Option parsing from RRQ/WRQ packets (case-insensitive, RFC 2347 compliant)
//! - OACK packet generation and transmission
//! - OptionNegotiation state for waiting on ACK block 0
//! - Timeout handling during option negotiation
//! - Error handling for option negotiation failures
//!
//! However, no specific options (blksize, tsize, etc.) are currently recognized.
//! The `negotiate_options` function returns an empty map for all inputs.
//!
//! To add support for a specific option (e.g., blksize):
//! 1. Update `negotiate_options` to recognize and validate the option
//! 2. Modify the transfer logic to use the negotiated option value
//! 3. Add comprehensive tests for the new option

use std::{net::SocketAddr, sync::Arc};

use log::{debug, warn};

use crate::tftp::{
    options::TftpOption,
    packet::{Error, Packet},
};
use anyhow::Result;

const DEFAULT_MAX_RETRIES: u8 = 4;

pub trait Handler {
    type Reader: Reader + Send + Sync;
    fn create_reader(
        &self,
        filename: &str,
        block_size: u64,
    ) -> impl Future<Output = Result<Self::Reader>> + Send;
    fn filesize(&self, filename: &str) -> impl Future<Output = Result<u64>> + Send;
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
    OptionNegotiation {
        filename: String,
        mode: String,
        negotiated_options: Vec<TftpOption>,
        timeouts: u8,
    },
    Reading {
        filename: String,
        mode: String,
        block: u16,
        reader: H::Reader,
        data: Vec<u8>,
        timeouts: u8,
        block_size: u64,
    },
    Complete,
}

// The State struct holds the current state of a TFTP transfer, including the address of the client, the current transfer state, and a reference to the handler.
pub struct State<H: Handler> {
    addr: SocketAddr,
    state: TransferState<H>,
    handler: Arc<H>,
    max_timeouts: u8,
}

impl<H: Handler + 'static> State<H> {
    // Creates a new State instance with the given address and handler.
    pub fn new(addr: SocketAddr, handler: Arc<H>) -> Self {
        Self {
            addr,
            state: TransferState::Uninitialized,
            handler,
            max_timeouts: DEFAULT_MAX_RETRIES,
        }
    }

    // Handle a TFTP packet, transitioning between states as necessary.
    pub async fn handle(&mut self, packet: Packet) -> ControlFlow {
        let result = match &mut self.state {
            TransferState::Uninitialized => match packet {
                Packet::Rrq {
                    filename,
                    mode,
                    options,
                } => handle_read_request(self.handler.as_ref(), filename, mode, options).await,
                _ => self.error(None),
            },
            TransferState::OptionNegotiation {
                filename,
                mode,
                negotiated_options,
                ..
            } => match packet {
                Packet::Ack { block: 0 } => {
                    handle_option_ack_with_state(
                        self.handler.as_ref(),
                        filename,
                        mode,
                        negotiated_options,
                    )
                    .await
                }
                Packet::Error { code, message } => {
                    log::debug!(
                        "TFTP: Received error packet for {}: {:?} - {}",
                        self.addr,
                        code,
                        message
                    );
                    self.close()
                }
                _ => self.error(None),
            },
            TransferState::Reading {
                filename,
                mode,
                block,
                reader,
                data,
                timeouts,
                block_size,
            } => match packet {
                Packet::Ack { block: acked_block } => {
                    handle_ack(
                        reader,
                        mode,
                        block,
                        data,
                        timeouts,
                        *block_size,
                        acked_block,
                    )
                    .await
                }
                Packet::Error { code, message } => {
                    log::debug!(
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
        match &mut self.state {
            TransferState::Uninitialized => {
                log::warn!("TFTP: Timeout in Uninitialized state for {}", self.addr);
                ControlFlow::Closed(None)
            }
            TransferState::OptionNegotiation {
                negotiated_options,
                timeouts,
                ..
            } => {
                *timeouts += 1;
                if *timeouts == self.max_timeouts {
                    log::warn!(
                        "TFTP: Abandoning transfer for too many option negotiation timeouts"
                    );
                    return ControlFlow::Closed(None);
                }
                // Retransmit OACK
                ControlFlow::Continue(Packet::Oack {
                    options: negotiated_options.clone(),
                })
            }
            TransferState::Reading {
                data,
                block,
                timeouts,
                ..
            } => {
                *timeouts += 1;
                if *timeouts == self.max_timeouts {
                    log::warn!("TFTP: Abandoning transfer for too many read timeouts");
                    return ControlFlow::Closed(None);
                }
                ControlFlow::Continue(Packet::Data {
                    block: *block,
                    data: data.clone(),
                })
            }
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

// Negotiate which options are acceptable
async fn negotiate_options<H: Handler>(
    handler: &H,
    filename: &str,
    options: Vec<TftpOption>,
) -> Result<Vec<TftpOption>> {
    let mut negotiated_options: Vec<TftpOption> = Vec::new();

    for opt in options {
        match opt {
            TftpOption::TSize(_) => {
                let filelen = handler.filesize(filename).await?;
                negotiated_options.push(TftpOption::TSize(filelen));
            }
            TftpOption::BlkSize(size) => {
                if size >= 10000 {
                    warn!("TFTP: Rejecting blksize over 10,000 ({})", size);
                } else {
                    negotiated_options.push(TftpOption::BlkSize(size));
                }
            }
            TftpOption::Unrecognized(key, _) => warn!("TFTP: Ignoring unrecognized option {}", key),
        }
    }

    Ok(negotiated_options)
}

// Handles an RRQ (Read Request) packet by initiating a read operation with the handler.
// If options are present, negotiates them and transitions to OptionNegotiation state.
// Otherwise, starts reading immediately.
async fn handle_read_request<H: Handler>(
    handler: &H,
    filename: String,
    mode: String,
    options: Vec<TftpOption>,
) -> Result<HandleResponse<H>> {
    // Negotiate options
    let negotiated_options = match negotiate_options(handler, &filename, options).await {
        Ok(options) => options,
        Err(_) => {
            return Ok(HandleResponse {
                next_state: Some(TransferState::Complete),
                response: ControlFlow::Closed(Some(Packet::Error {
                    code: Error::FileNotFound,
                    message: "".to_owned(),
                })),
            });
        }
    };

    if !negotiated_options.is_empty() {
        // Options were negotiated - send OACK and wait for ACK block 0
        let next_state = TransferState::OptionNegotiation {
            filename,
            mode,
            negotiated_options: negotiated_options.clone(),
            timeouts: 0,
        };
        let reply = Packet::Oack {
            options: negotiated_options,
        };
        Ok(HandleResponse {
            next_state: Some(next_state),
            response: ControlFlow::Continue(reply),
        })
    } else {
        // No options or no options negotiated - start transfer immediately
        // Per RFC 1350, block numbers begin with one
        let mut reader = handler.create_reader(&filename, 512).await?;
        let data = reader.read().await?;
        let next_state = TransferState::Reading {
            filename,
            mode,
            block: 1,
            reader,
            data: data.clone(),
            timeouts: 0,
            block_size: 512,
        };
        let reply = Packet::Data { block: 1, data };
        Ok(HandleResponse {
            next_state: Some(next_state),
            response: ControlFlow::Continue(reply),
        })
    }
}

// Handles ACK block 0 after sending OACK.
// Takes ownership of reader and transitions to Reading state.
// Per RFC 1350, block numbers begin with one, so we send DATA block 1.
async fn handle_option_ack_with_state<H: Handler>(
    handler: &H,
    filename: &str,
    mode: &str,
    negotiated_options: &Vec<TftpOption>,
) -> Result<HandleResponse<H>> {
    let mut block_size: u64 = 512;
    for opt in negotiated_options {
        if let TftpOption::BlkSize(size) = opt {
            block_size = *size;
            break;
        }
    }

    // Client acknowledged the options - start sending data at block 1
    let mut reader = handler.create_reader(filename, block_size).await?;
    let data = reader.read().await?;

    let next_state = TransferState::Reading {
        filename: filename.to_owned(),
        mode: mode.to_owned(),
        block: 1,
        reader,
        data: data.clone(),
        timeouts: 0,
        block_size,
    };

    let reply = Packet::Data { block: 1, data };
    Ok(HandleResponse {
        next_state: Some(next_state),
        response: ControlFlow::Continue(reply),
    })
}

// Handles an ACK (Acknowledgment) packet by updating the block number and reading the next data chunk.
//
// Per RFC 1350, block numbers begin with one:
// - Client sends ACK 1 to acknowledge DATA block 1
// - Server responds with DATA block 2
// - Client sends ACK 2 to acknowledge DATA block 2, etc.
async fn handle_ack<H: Handler>(
    reader: &mut H::Reader,
    _mode: &str,
    block: &mut u16,
    data: &mut Vec<u8>,
    timeouts: &mut u8,
    block_size: u64,
    acked_block: u16,
) -> Result<HandleResponse<H>> {
    let next_block = block.wrapping_add(1);
    let data = if acked_block == *block {
        // If the ACK is for the current block, and the current block is less than the negotiated
        // block size, the transfer is complete.
        if data.len() < block_size as usize {
            debug!("TFTP: Transfer complete for block {acked_block}");
            return Ok(HandleResponse {
                next_state: Some(TransferState::Complete),
                response: ControlFlow::Closed(None),
            });
        }
        // Otherwise, read the next block of data.
        *timeouts = 0;
        *block = next_block;
        *data = reader.read().await?;
        data.clone()
    } else if acked_block == block.wrapping_sub(1) {
        // If the ACK is for the previous block, resend the current block.
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

        async fn create_reader(&self, _filename: &str, block_size: u64) -> Result<Self::Reader> {
            Ok(MockReader {
                data: self
                    .data
                    .chunks(block_size as usize)
                    .map(|x| x.into())
                    .collect(),
                next_block: 0,
            })
        }

        async fn filesize(&self, _filename: &str) -> Result<u64> {
            Ok(self.data.len() as u64)
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
                options: Vec::new(),
            })
            .await;

        // Per RFC 1350, first DATA packet should be block 1
        assert!(
            matches!(result, ControlFlow::Continue(packet::Packet::Data { block: 1, ref data }) if data == &vec![0; 512]),
            "Got response {result:?}"
        );

        // Simulate ACK 1 for the first block (DATA block 1)
        let result = state.handle(Packet::Ack { block: 1 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(packet::Packet::Data { block: 2, ref data }) if data == &vec![0; 1]),
            "Got response {result:?}"
        );

        // Simulate ACK 2 for the second block (DATA block 2)
        let result = state.handle(Packet::Ack { block: 2 }).await;
        assert!(
            matches!(result, ControlFlow::Closed(None)),
            "Got response {result:?}"
        );
    }

    #[tokio::test]
    async fn timeout_exceeds_attempts() {
        // Simulate TFTP read request
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 513])),
        );

        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;
        assert!(matches!(result, ControlFlow::Continue(_)));

        // First few timeouts
        for _ in 1..DEFAULT_MAX_RETRIES {
            let result = state.handle_timeout().await;
            assert!(matches!(result, ControlFlow::Continue(_)));
        }

        // Last timeout should give up
        let result = state.handle_timeout().await;
        assert!(matches!(result, ControlFlow::Closed(_)));
    }

    // Tests for RFC 2347 Option Negotiation

    #[tokio::test]
    async fn test_rrq_without_options_bypasses_negotiation() {
        // RRQ without options should skip negotiation and send DATA immediately
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 100])),
        );

        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;

        // Per RFC 1350, should receive DATA block 1, not OACK
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, .. })),
            "Expected DATA block 1 without options, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_rrq_with_unrecognized_options_bypasses_negotiation() {
        // RRQ with options we don't recognize should skip negotiation
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 100])),
        );

        let options = vec![TftpOption::Unrecognized("Foo".to_owned(), "Bar".to_owned())];

        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options,
            })
            .await;

        // Per RFC 1350, should receive DATA block 1, not OACK (we don't recognize any options yet)
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, .. })),
            "Expected DATA block 1 for unrecognized options, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_option_negotiation_retransmits_oack_on_timeout() {
        // When in OptionNegotiation state, timeout should retransmit OACK
        // This test will be relevant once we implement specific options
        // For now, we can't test this since no options are recognized
        // TODO: Enable this test when we implement blksize or another option
    }

    #[tokio::test]
    async fn test_option_negotiation_closes_after_max_timeouts() {
        // When in OptionNegotiation state, should close after max timeouts
        // This test will be relevant once we implement specific options
        // For now, we can't test this since no options are recognized
        // TODO: Enable this test when we implement blksize or another option
    }

    // Test helper: MockHandlerWithOptions that simulates option support
    // This demonstrates how option negotiation will work once we implement specific options
    #[tokio::test]
    async fn test_block_numbering_starts_at_one() {
        // Test that block numbering follows RFC 1350: blocks start at 1, not 0
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![1; 512])),
        );

        // Send RRQ
        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;

        // Should receive DATA block 1 (not block 0)
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, ref data }) if data.len() == 512),
            "First DATA packet should be block 1, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_ack_sequence_follows_data_blocks() {
        // Test that ACK numbers match DATA block numbers
        // Use 1025 bytes = 2 full blocks (512 each) + 1 partial block (1 byte)
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 1025])),
        );

        // RRQ -> DATA block 1 (512 bytes)
        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, ref data }) if data.len() == 512),
            "First DATA packet should be block 1 with 512 bytes, got {result:?}"
        );

        // ACK 1 -> DATA block 2 (512 bytes)
        let result = state.handle(Packet::Ack { block: 1 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 2, ref data }) if data.len() == 512),
            "After ACK 1, should receive DATA block 2 with 512 bytes, got {result:?}"
        );

        // ACK 2 -> DATA block 3 (1 byte - final block)
        let result = state.handle(Packet::Ack { block: 2 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 3, ref data }) if data.len() == 1),
            "After ACK 2, should receive DATA block 3 with 1 byte, got {result:?}"
        );

        // ACK 3 -> Complete (last block was < 512 bytes)
        let result = state.handle(Packet::Ack { block: 3 }).await;
        assert!(
            matches!(result, ControlFlow::Closed(None)),
            "After ACK 3 for final block, transfer should complete, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_retransmit_on_duplicate_ack() {
        // Test that receiving a duplicate ACK causes retransmission of current block
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 1024])),
        );

        // RRQ -> DATA block 1
        state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;

        // ACK 1 -> DATA block 2
        state.handle(Packet::Ack { block: 1 }).await;

        // Duplicate ACK 1 -> should retransmit DATA block 2
        let result = state.handle(Packet::Ack { block: 1 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 2, .. })),
            "Duplicate ACK 1 should retransmit DATA block 2, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_option_negotiation_followed_by_block_one() {
        // When option negotiation would occur (if we supported options),
        // the flow should be: RRQ -> OACK -> ACK 0 -> DATA block 1
        // This test documents expected future behavior
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 100])),
        );

        let options = vec![TftpOption::BlkSize(1024)];

        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: options.clone(),
            })
            .await;

        // Currently we don't recognize options, so we get DATA block 1 immediately
        // In the future with option support:
        // - Should get OACK
        // - Send ACK 0
        // - Receive DATA block 1 (NOT block 0)
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Oack { options: opt }) if opt == options),
            "Must return Oack"
        );

        let result2 = state.handle(Packet::Ack { block: 0 }).await;
        assert!(
            matches!(
                result2,
                ControlFlow::Continue(Packet::Data { block: 1, .. })
            ),
            "Oack followed by Ack 0 must be followed by Data block 1"
        );
    }

    #[tokio::test]
    async fn test_unexpected_ack_returns_error() {
        // Test that receiving an out-of-sequence ACK causes an error
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 1024])),
        );

        // RRQ -> DATA block 1
        state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: Vec::new(),
            })
            .await;

        // Send ACK 5 (out of sequence - we're expecting ACK 1)
        let result = state.handle(Packet::Ack { block: 5 }).await;
        assert!(
            matches!(result, ControlFlow::Closed(Some(Packet::Error { .. }))),
            "Unexpected ACK should return error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_future_option_negotiation_flow() {
        // This test documents the expected flow when we implement option support
        // Currently all options return empty, but the framework is in place

        // Step 1: Create a mock that would recognize options (future implementation)
        // For now, this will behave like no options are recognized
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 1024])),
        );

        // Step 2: Send RRQ with options
        let options = vec![TftpOption::BlkSize(1024), TftpOption::TSize(0)];

        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options: options.clone(),
            })
            .await;

        // Step 3: Currently, since we don't recognize options, should get DATA block 1
        // In the future, when blksize is implemented, should get OACK
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Oack { options }) if options == vec![TftpOption::BlkSize(1024), TftpOption::TSize(1024)]),
            "Expected Oack packet with filled in blksize and tsize"
        );

        // Step 4: Client sends ACK block 0 to accept options
        let result = state.handle(Packet::Ack { block: 0 }).await;
        // Per RFC 1350, after ACK 0, should receive DATA block 1
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, .. })),
            "After ACK 0, should receive DATA block 1"
        );
    }

    #[tokio::test]
    async fn test_completion_with_negotiated_block_size() {
        // Test that transfer completes correctly when using a negotiated block size > 512
        // This verifies the fix for the completion detection bug

        // File size: 1600 bytes
        // Negotiated blksize: 1024
        // Expected blocks: Block 1 (1024 bytes), Block 2 (576 bytes - final)
        // After ACK 2, transfer should complete (576 < 1024)
        let mut state = State::new(
            SocketAddr::from_str("127.0.0.1:55").unwrap(),
            Arc::new(MockHandler::with_data(vec![0; 1600])),
        );

        // Send RRQ with blksize option
        let options = vec![TftpOption::BlkSize(1024)];
        let result = state
            .handle(Packet::Rrq {
                filename: String::from("test.txt"),
                mode: String::from("octet"),
                options,
            })
            .await;

        // Should receive OACK with negotiated blksize
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Oack { .. })),
            "Should receive OACK for blksize negotiation, got {result:?}"
        );

        // ACK 0 -> DATA block 1 (1024 bytes)
        let result = state.handle(Packet::Ack { block: 0 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 1, ref data }) if data.len() == 1024),
            "After ACK 0, should receive DATA block 1 with 1024 bytes, got {result:?}"
        );

        // ACK 1 -> DATA block 2 (576 bytes - final block)
        let result = state.handle(Packet::Ack { block: 1 }).await;
        assert!(
            matches!(result, ControlFlow::Continue(Packet::Data { block: 2, ref data }) if data.len() == 576),
            "After ACK 1, should receive DATA block 2 with 576 bytes, got {result:?}"
        );

        // ACK 2 -> Complete (576 < 1024, so transfer should complete)
        // This is the critical test - without the fix, this would NOT complete
        // because it would check 576 < 512 (false) instead of 576 < 1024 (true)
        let result = state.handle(Packet::Ack { block: 2 }).await;
        assert!(
            matches!(result, ControlFlow::Closed(None)),
            "After ACK 2 for final block (576 < 1024), transfer should complete, got {result:?}"
        );
    }
}
