use anyhow::{Result, anyhow};
use log::warn;

use crate::tftp::options::{Error as TftpError, TftpOption};

#[derive(Debug, PartialEq)]
pub enum Error {
    Undefined,
    FileNotFound,
    AccessViolation,
    DiskFull,
    IllegalOperation,
    UnknownTransferID,
    FileAlreadyExists,
    NoSuchUser,
    OptionNegotiationFailure,
    Unknown { code: u16 },
}

impl From<u16> for Error {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Undefined,
            1 => Self::FileNotFound,
            2 => Self::AccessViolation,
            3 => Self::DiskFull,
            4 => Self::IllegalOperation,
            5 => Self::UnknownTransferID,
            6 => Self::FileAlreadyExists,
            7 => Self::NoSuchUser,
            8 => Self::OptionNegotiationFailure,
            _ => Self::Unknown { code: value },
        }
    }
}

impl From<Error> for u16 {
    fn from(value: Error) -> Self {
        match value {
            Error::Undefined => 0,
            Error::FileNotFound => 1,
            Error::AccessViolation => 2,
            Error::DiskFull => 3,
            Error::IllegalOperation => 4,
            Error::UnknownTransferID => 5,
            Error::FileAlreadyExists => 6,
            Error::NoSuchUser => 7,
            Error::OptionNegotiationFailure => 8,
            Error::Unknown { code } => code,
        }
    }
}

impl From<&Error> for u16 {
    fn from(value: &Error) -> Self {
        match value {
            Error::Undefined => 0,
            Error::FileNotFound => 1,
            Error::AccessViolation => 2,
            Error::DiskFull => 3,
            Error::IllegalOperation => 4,
            Error::UnknownTransferID => 5,
            Error::FileAlreadyExists => 6,
            Error::NoSuchUser => 7,
            Error::OptionNegotiationFailure => 8,
            Error::Unknown { code } => *code,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Packet {
    Rrq {
        filename: String,
        mode: String,
        options: Vec<TftpOption>,
    },
    Wrq {
        filename: String,
        mode: String,
        options: Vec<TftpOption>,
    },
    Data {
        block: u16,
        data: Vec<u8>,
    },
    Ack {
        block: u16,
    },
    Error {
        code: Error,
        message: String,
    },
    Oack {
        options: Vec<TftpOption>,
    },
}

impl Packet {
    pub fn parse(data: &[u8]) -> Result<Packet> {
        let (opcode, remainder) = parse_opcode(data)?;

        match opcode {
            1 => parse_rrq(remainder),
            2 => parse_wrq(remainder),
            3 => parse_data(remainder),
            4 => parse_ack(remainder),
            5 => parse_error(remainder),
            6 => parse_oack(remainder),
            _ => Err(anyhow!("unknown opcode {opcode}")),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        match self {
            Packet::Rrq {
                filename,
                mode,
                options,
            } => {
                write_u16(&mut bytes, 1);
                write_string(&mut bytes, filename);
                write_string(&mut bytes, mode);
                write_options(&mut bytes, options);
            }
            Packet::Wrq {
                filename,
                mode,
                options,
            } => {
                write_u16(&mut bytes, 2);
                write_string(&mut bytes, filename);
                write_string(&mut bytes, mode);
                write_options(&mut bytes, options);
            }
            Packet::Data { block, data } => {
                write_u16(&mut bytes, 3);
                write_u16(&mut bytes, *block);
                bytes.extend_from_slice(data);
            }
            Packet::Ack { block } => {
                write_u16(&mut bytes, 4);
                write_u16(&mut bytes, *block);
            }
            Packet::Error { code, message } => {
                write_u16(&mut bytes, 5);
                write_u16(&mut bytes, code.into());
                write_string(&mut bytes, message);
            }
            Packet::Oack { options } => {
                write_u16(&mut bytes, 6);
                write_options(&mut bytes, options);
            }
        };

        bytes
    }
}

fn parse_opcode(data: &[u8]) -> Result<(u16, &[u8])> {
    read_u16(data)
}

fn parse_rrq(data: &[u8]) -> Result<Packet> {
    let (filename, remainder) = read_string(data)?;
    let (mode, remainder) = read_string(remainder)?;
    let options = parse_options(remainder)?;

    Ok(Packet::Rrq {
        filename,
        mode,
        options,
    })
}

fn parse_wrq(data: &[u8]) -> Result<Packet> {
    let (filename, remainder) = read_string(data)?;
    let (mode, remainder) = read_string(remainder)?;
    let options = parse_options(remainder)?;

    Ok(Packet::Wrq {
        filename,
        mode,
        options,
    })
}

fn parse_data(data: &[u8]) -> Result<Packet> {
    let (block, remainder) = read_u16(data)?;
    Ok(Packet::Data {
        block,
        data: remainder.into(),
    })
}

fn parse_ack(data: &[u8]) -> Result<Packet> {
    let (block, remainder) = read_u16(data)?;
    if !remainder.is_empty() {
        warn!("TFTP bytes remaining after parsing ACK packet");
    }

    Ok(Packet::Ack { block })
}

fn parse_error(data: &[u8]) -> Result<Packet> {
    let (code, remainder) = read_u16(data)?;
    let (message, remainder) = read_string(remainder)?;
    if !remainder.is_empty() {
        warn!("TFTP bytes remaining after parsing ERROR packet");
    }

    Ok(Packet::Error {
        code: code.into(),
        message,
    })
}

fn read_u16(data: &[u8]) -> Result<(u16, &[u8])> {
    if let Some((opcode_bytes, remainder)) = data.split_at_checked(size_of::<u16>()) {
        let opcode = u16::from_be_bytes(opcode_bytes.try_into().unwrap());
        Ok((opcode, remainder))
    } else {
        Err(anyhow!("packet not long enough for reading u16"))
    }
}

fn read_string(data: &[u8]) -> Result<(String, &[u8])> {
    if let Some(terminator_position) = data.iter().position(|&c| c == b'\0') {
        let str_bytes = &data[0..terminator_position];
        let remainder = &data[terminator_position + 1..];
        let string = String::from_utf8_lossy(str_bytes).into_owned();
        Ok((string, remainder))
    } else {
        Err(anyhow!("packet missing null terminator reading string"))
    }
}

fn write_u16(buf: &mut Vec<u8>, data: u16) {
    buf.extend_from_slice(&data.to_be_bytes());
}

fn write_string<T: AsRef<str>>(buf: &mut Vec<u8>, data: T) {
    buf.extend_from_slice(data.as_ref().as_bytes());
    buf.push(b'\0');
}

fn parse_oack(data: &[u8]) -> Result<Packet> {
    let options = parse_options(data)?;
    Ok(Packet::Oack { options })
}

// Parses TFTP options from the remainder of a packet.
// Options are key-value pairs, each null-terminated.
// Keys are normalized to lowercase per RFC 2347 (case-insensitive).
// Returns an error if an option key appears more than once (RFC 2347 requirement).
fn parse_options(mut data: &[u8]) -> Result<Vec<TftpOption>> {
    let mut options = Vec::new();

    while !data.is_empty() {
        let (key, remainder) = read_string(data)?;
        let (value, remainder) = read_string(remainder)?;

        // Normalize key to lowercase for case-insensitive comparison
        let key_lower = key.to_lowercase();

        let option = match TftpOption::from_pair(key_lower, value) {
            Ok(option) => option,
            Err(TftpError::InvalidValue) => {
                log::warn!("TFTP: Invalid value parsing options");
                continue;
            }
        };

        // RFC 2347: options may only be specified once
        if options_contain_duplicate(&options, &option) {
            return Err(anyhow!("duplicate option key: {}", key));
        }

        options.push(option);
        data = remainder;
    }

    Ok(options)
}

fn options_contain_duplicate(options: &[TftpOption], option: &TftpOption) -> bool {
    // If the Option is Unrecognized, find other Unrecognized with the same key.
    // Otherwise, match on the Option type only
    match option {
        TftpOption::Unrecognized(k, _) => options
            .iter()
            .any(|o| matches!(o, TftpOption::Unrecognized(k2, _) if k == k2)),
        _ => options
            .iter()
            .any(|o| std::mem::discriminant(o) == std::mem::discriminant(option)),
    }
}

// Writes TFTP options to a buffer as null-terminated key-value pairs.
fn write_options(buf: &mut Vec<u8>, options: &Vec<TftpOption>) {
    for opt in options {
        let (key, value) = opt.to_pair();
        write_string(buf, key);
        write_string(buf, &value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16() {
        let bytes = [0x00, 0xFF, 0x00];
        let result = read_u16(&bytes[0..3]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (255_u16, &bytes[2..3]));
    }

    #[test]
    fn test_read_u16_empty() {
        let bytes = [];
        let result = read_u16(&bytes[0..0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_string_empty() {
        let bytes = [];
        let result = read_string(&bytes[0..0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_string_no_terminator() {
        let bytes = [b'A', b'B'];
        let result = read_string(&bytes[0..2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_string() {
        let bytes = [b'A', b'B', b'\0'];
        let result = read_string(&bytes[0..3]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ("AB".to_owned(), &bytes[3..3]));
    }

    #[test]
    fn test_read_string_remainder() {
        let bytes = [b'A', b'B', b'\0', b'X'];
        let result = read_string(&bytes[0..4]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ("AB".to_owned(), &bytes[3..4]));
    }

    #[test]
    fn test_parse_rrq() {
        let bytes = [0x00, 0x01, b'A', b'A', b'\0', b'B', b'B', b'\0'];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Packet::Rrq {
                filename: "AA".to_owned(),
                mode: "BB".to_owned(),
                options: Vec::new(),
            }
        );
    }

    #[test]
    fn test_parse_wrq() {
        let bytes = [0x00, 0x02, b'A', b'A', b'\0', b'B', b'B', b'\0'];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Packet::Wrq {
                filename: "AA".to_owned(),
                mode: "BB".to_owned(),
                options: Vec::new(),
            }
        );
    }

    #[test]
    fn test_parse_data() {
        let bytes = [0x00, 0x03, 0x00, 0x01, b'B', b'B'];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Packet::Data {
                block: 1,
                data: b"BB".to_vec()
            }
        );
    }

    #[test]
    fn test_parse_ack() {
        let bytes = [0x00, 0x04, 0x00, 0x01];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Packet::Ack { block: 1 });
    }

    #[test]
    fn test_parse_err() {
        let bytes = [0x00, 0x05, 0x00, 0x02, b'B', b'B', b'\0'];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Packet::Error {
                code: Error::AccessViolation,
                message: "BB".to_owned()
            }
        );
    }

    // Tests for RFC 2347 Option Extension

    #[test]
    fn test_parse_rrq_with_options() {
        // RRQ with filename "test.txt", mode "octet", and option "blksize" = "1024"
        let bytes = [
            0x00, 0x01, // opcode 1 (RRQ)
            b't', b'e', b's', b't', b'.', b't', b'x', b't', b'\0', // filename
            b'o', b'c', b't', b'e', b't', b'\0', // mode
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', // option key
            b'1', b'0', b'2', b'4', b'\0', // option value
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        let packet = result.unwrap();
        match packet {
            Packet::Rrq {
                filename,
                mode,
                options,
            } => {
                assert_eq!(filename, "test.txt");
                assert_eq!(mode, "octet");
                assert_eq!(options.len(), 1);
                assert_eq!(options[0], TftpOption::BlkSize(1024));
            }
            _ => panic!("Expected RRQ packet"),
        }
    }

    #[test]
    fn test_parse_rrq_with_multiple_options() {
        // RRQ with two options: blksize and tsize
        let bytes = [
            0x00, 0x01, // opcode 1 (RRQ)
            b'f', b'i', b'l', b'e', b'\0', // filename
            b'o', b'c', b't', b'e', b't', b'\0', // mode
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', // option 1 key
            b'5', b'1', b'2', b'\0', // option 1 value
            b't', b's', b'i', b'z', b'e', b'\0', // option 2 key
            b'1', b'0', b'0', b'0', b'\0', // option 2 value
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        let packet = result.unwrap();
        match packet {
            Packet::Rrq {
                filename,
                mode,
                options,
            } => {
                assert_eq!(filename, "file");
                assert_eq!(mode, "octet");
                assert_eq!(
                    options,
                    vec![TftpOption::BlkSize(512), TftpOption::TSize(1000)]
                );
            }
            _ => panic!("Expected RRQ packet"),
        }
    }

    #[test]
    fn test_parse_rrq_options_case_insensitive() {
        // Test that option keys are normalized to lowercase
        let bytes = [
            0x00, 0x01, // opcode 1 (RRQ)
            b'f', b'i', b'l', b'e', b'\0', // filename
            b'o', b'c', b't', b'e', b't', b'\0', // mode
            b'B', b'L', b'K', b'S', b'I', b'Z', b'E', b'\0', // option key (uppercase)
            b'1', b'0', b'2', b'4', b'\0', // option value
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        let packet = result.unwrap();
        match packet {
            Packet::Rrq { options, .. } => {
                // Key should be normalized to lowercase
                assert_eq!(options, vec![TftpOption::BlkSize(1024)]);
            }
            _ => panic!("Expected RRQ packet"),
        }
    }

    #[test]
    fn test_parse_rrq_duplicate_option_fails() {
        // RFC 2347: options may only be specified once
        let bytes = [
            0x00, 0x01, // opcode 1 (RRQ)
            b'f', b'i', b'l', b'e', b'\0', // filename
            b'o', b'c', b't', b'e', b't', b'\0', // mode
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', // option key
            b'5', b'1', b'2', b'\0', // option value
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', // duplicate option key
            b'1', b'0', b'2', b'4', b'\0', // different value
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate option"));
    }

    #[test]
    fn test_parse_oack() {
        // OACK with blksize option
        let bytes = [
            0x00, 0x06, // opcode 6 (OACK)
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', // option key
            b'1', b'0', b'2', b'4', b'\0', // option value
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        match result.unwrap() {
            Packet::Oack { options } => {
                assert_eq!(options, vec![TftpOption::BlkSize(1024)]);
            }
            _ => panic!("Expected OACK packet"),
        }
    }

    #[test]
    fn test_parse_oack_empty() {
        // OACK with no options (valid but unusual)
        let bytes = [0x00, 0x06]; // opcode 6 (OACK), no options
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        match result.unwrap() {
            Packet::Oack { options } => {
                assert_eq!(options.len(), 0);
            }
            _ => panic!("Expected OACK packet"),
        }
    }

    #[test]
    fn test_parse_oack_multiple_options() {
        // OACK with multiple options
        let bytes = [
            0x00, 0x06, // opcode 6 (OACK)
            b'b', b'l', b'k', b's', b'i', b'z', b'e', b'\0', b'5', b'1', b'2', b'\0', b't', b's',
            b'i', b'z', b'e', b'\0', b'1', b'0', b'0', b'0', b'\0',
        ];
        let result = Packet::parse(&bytes[..]);
        assert!(result.is_ok());

        match result.unwrap() {
            Packet::Oack { options } => {
                assert_eq!(
                    options,
                    vec![TftpOption::BlkSize(512), TftpOption::TSize(1000)]
                );
            }
            _ => panic!("Expected OACK packet"),
        }
    }

    #[test]
    fn test_to_bytes_rrq_with_options() {
        let mut options = Vec::new();
        options.push(TftpOption::BlkSize(1024));

        let packet = Packet::Rrq {
            filename: "test.txt".to_string(),
            mode: "octet".to_string(),
            options,
        };

        let bytes = packet.to_bytes();
        let parsed = Packet::parse(&bytes).unwrap();

        assert_eq!(packet, parsed);
    }

    #[test]
    fn test_to_bytes_oack() {
        let mut options = Vec::new();
        options.push(TftpOption::BlkSize(1024));
        options.push(TftpOption::TSize(5000));

        let packet = Packet::Oack { options };
        let bytes = packet.to_bytes();
        let parsed = Packet::parse(&bytes).unwrap();

        assert_eq!(packet, parsed);
    }

    #[test]
    fn test_roundtrip_rrq_no_options() {
        let packet = Packet::Rrq {
            filename: "file.bin".to_string(),
            mode: "octet".to_string(),
            options: Vec::new(),
        };

        let bytes = packet.to_bytes();
        let parsed = Packet::parse(&bytes).unwrap();

        assert_eq!(packet, parsed);
    }

    #[test]
    fn test_error_code_option_negotiation_failure() {
        // Test error code 8 conversion
        let error = Error::from(8u16);
        assert_eq!(error, Error::OptionNegotiationFailure);

        let code: u16 = error.into();
        assert_eq!(code, 8);
    }

    #[test]
    fn test_to_bytes_error_option_negotiation() {
        let packet = Packet::Error {
            code: Error::OptionNegotiationFailure,
            message: "Option not supported".to_string(),
        };

        let bytes = packet.to_bytes();
        let parsed = Packet::parse(&bytes).unwrap();

        assert_eq!(packet, parsed);
    }
}
