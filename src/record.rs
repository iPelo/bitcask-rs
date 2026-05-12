/// Bytes in the fixed-size record header:
/// crc32 + timestamp + key_size + value_size.
pub const HEADER_LEN: usize = 4 + 8 + 4 + 4;

/// Value-size sentinel used to represent a deleted key.
pub const TOMBSTONE_VALUE_SIZE: u32 = u32::MAX;

/// Fixed metadata that prefixes every on-disk record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordHeader {
    pub crc: u32,
    pub timestamp: u64,
    pub key_size: u32,
    pub value_size: u32,
}

impl RecordHeader {
    pub fn is_tombstone(&self) -> bool {
        self.value_size == TOMBSTONE_VALUE_SIZE
    }
}

/// Logical record before or after encoding to the append-only log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub timestamp: u64,
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
}

impl Record {
    pub fn put(timestamp: u64, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            timestamp,
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn tombstone(timestamp: u64, key: impl Into<Vec<u8>>) -> Self {
        Self {
            timestamp,
            key: key.into(),
            value: None,
        }
    }
}

