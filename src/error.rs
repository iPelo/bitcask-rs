use std::fmt;
use std::io;

/// Result type used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the storage engine.
#[derive(Debug)]
pub enum Error {
    /// An underlying filesystem operation failed.
    Io(io::Error),
    /// A record on disk could not be parsed (e.g. a short or truncated header).
    CorruptRecord(String),
    /// A record's stored checksum did not match its contents.
    CrcMismatch { expected: u32, actual: u32 },
    /// A key or value is too long to be stored in the record format.
    TooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::CorruptRecord(message) => write!(f, "corrupt record: {message}"),
            Self::CrcMismatch { expected, actual } => {
                write!(f, "crc mismatch: expected {expected}, got {actual}")
            }
            Self::TooLarge { field, len, max } => {
                write!(f, "{field} too large: {len} bytes (max {max})")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::CorruptRecord(_) | Self::CrcMismatch { .. } | Self::TooLarge { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
