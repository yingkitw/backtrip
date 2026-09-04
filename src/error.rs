use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    InvalidPe(String),
    InvalidMetadata(String),
    InvalidCil(String),
    InvalidSignature(String),
    NotImplemented(&'static str),
    NotFound(String),
    Usage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::InvalidPe(m) => write!(f, "invalid PE: {m}"),
            Error::InvalidMetadata(m) => write!(f, "invalid metadata: {m}"),
            Error::InvalidCil(m) => write!(f, "invalid CIL: {m}"),
            Error::InvalidSignature(m) => write!(f, "invalid signature: {m}"),
            Error::NotImplemented(m) => write!(f, "not implemented: {m}"),
            Error::NotFound(m) => write!(f, "not found: {m}"),
            Error::Usage(m) => write!(f, "usage: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
