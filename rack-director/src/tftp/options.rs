pub enum Error {
    InvalidValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TftpOption {
    TSize(u64),
    BlkSize(u64),
    Unrecognized(String, String),
}

impl TftpOption {
    pub fn from_pair<T: AsRef<str>>(key: T, value: T) -> Result<TftpOption, Error> {
        match key.as_ref() {
            "tsize" => {
                let v: u64 = value.as_ref().parse().map_err(|_| Error::InvalidValue)?;
                Ok(TftpOption::TSize(v))
            }
            "blksize" => {
                let v: u64 = value.as_ref().parse().map_err(|_| Error::InvalidValue)?;
                Ok(TftpOption::BlkSize(v))
            }
            _ => Ok(TftpOption::Unrecognized(
                key.as_ref().to_owned(),
                value.as_ref().to_owned(),
            )),
        }
    }

    pub fn to_pair(&self) -> (&str, String) {
        match self {
            TftpOption::TSize(v) => ("tsize", v.to_string()),
            TftpOption::BlkSize(v) => ("blksize", v.to_string()),
            TftpOption::Unrecognized(key, value) => (key, value.to_owned()),
        }
    }
}
