use anyhow::{Result, anyhow};
use log::warn;

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
            Error::Unknown { code } => *code,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Packet {
    Rrq { filename: String, mode: String },
    Wrq { filename: String, mode: String },
    Data { block: u16, data: Vec<u8> },
    Ack { block: u16 },
    Error { code: Error, message: String },
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
            _ => Err(anyhow!("unknown opcode {opcode}")),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        match self {
            Packet::Rrq { filename, mode } => {
                write_u16(&mut bytes, 1);
                write_string(&mut bytes, filename);
                write_string(&mut bytes, mode);
            }
            Packet::Wrq { filename, mode } => {
                write_u16(&mut bytes, 2);
                write_string(&mut bytes, filename);
                write_string(&mut bytes, mode);
            }
            Packet::Data { block, data } => {
                write_u16(&mut bytes, 3);
                write_u16(&mut bytes, *block);
                bytes.extend_from_slice(data);
            }
            Packet::Ack { block } => {
                write_u16(&mut bytes, 3);
                write_u16(&mut bytes, *block);
            }
            Packet::Error { code, message } => {
                write_u16(&mut bytes, code.into());
                write_string(&mut bytes, message);
            }
        };

        bytes
    }

    pub fn can_initiate(&self) -> bool {
        match self {
            Packet::Rrq {
                filename: _,
                mode: _,
            } => true,
            Packet::Wrq {
                filename: _,
                mode: _,
            } => true,
            Packet::Data { block: _, data: _ } => false,
            Packet::Ack { block: _ } => false,
            Packet::Error {
                code: _,
                message: _,
            } => false,
        }
    }
}

fn parse_opcode(data: &[u8]) -> Result<(u16, &[u8])> {
    read_u16(data)
}

fn parse_rrq(data: &[u8]) -> Result<Packet> {
    let (filename, remainder) = read_string(data)?;
    let (mode, remainder) = read_string(remainder)?;
    if !remainder.is_empty() {
        warn!("TFTP bytes remaining after parsing RRQ packet")
    }

    Ok(Packet::Rrq { filename, mode })
}

fn parse_wrq(data: &[u8]) -> Result<Packet> {
    let (filename, remainder) = read_string(data)?;
    let (mode, remainder) = read_string(remainder)?;
    if !remainder.is_empty() {
        warn!("TFTP bytes remaining after parsing WRQ packet");
    }

    Ok(Packet::Wrq { filename, mode })
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

fn write_string(buf: &mut Vec<u8>, data: &String) {
    buf.extend_from_slice(data.as_bytes());
    buf.push(b'0');
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
                mode: "BB".to_owned()
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
                mode: "BB".to_owned()
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
}
