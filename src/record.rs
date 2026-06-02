//! On-disk record format and CRC checksum.
//!
//! Every record is laid out as a fixed header followed by the key and value
//! bytes:
//!
//! ```text
//! ┌───────┬───────────┬──────────┬────────────┬──────┬────────┐
//! │ CRC32 │ Timestamp │ Key size │ Value size │ Key  │ Value  │
//! │ 4 B   │   8 B     │   4 B    │    4 B     │ var  │  var   │
//! └───────┴───────────┴──────────┴────────────┴──────┴────────┘
//! ```
//!
//! Integers are stored little-endian. The CRC covers every byte after itself
//! (timestamp, sizes, key, value). A tombstone is encoded by setting the
//! value-size field to [`TOMBSTONE_VALUE_SIZE`] and writing no value bytes.

use std::sync::OnceLock;

use crate::error::{Error, Result};

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
    /// Whether this header describes a deleted key.
    pub fn is_tombstone(&self) -> bool {
        self.value_size == TOMBSTONE_VALUE_SIZE
    }

    /// Number of value bytes stored on disk (zero for a tombstone).
    pub fn value_len(&self) -> usize {
        if self.is_tombstone() {
            0
        } else {
            self.value_size as usize
        }
    }

    /// Total on-disk length of the record this header prefixes.
    pub fn record_len(&self) -> usize {
        HEADER_LEN + self.key_size as usize + self.value_len()
    }

    /// Parse a header from the first [`HEADER_LEN`] bytes of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::CorruptRecord(format!(
                "header needs {HEADER_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self {
            crc: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            timestamp: u64::from_le_bytes(bytes[4..12].try_into().unwrap()),
            key_size: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            value_size: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        })
    }
}

/// Logical record before or after encoding to the append-only log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub timestamp: u64,
    pub key: Vec<u8>,
    /// `Some` for a written value, `None` for a tombstone (deleted key).
    pub value: Option<Vec<u8>>,
}

impl Record {
    /// Create a record that sets `key` to `value`.
    pub fn put(timestamp: u64, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            timestamp,
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// Create a tombstone record that marks `key` as deleted.
    pub fn tombstone(timestamp: u64, key: impl Into<Vec<u8>>) -> Self {
        Self {
            timestamp,
            key: key.into(),
            value: None,
        }
    }

    /// Whether this record marks its key as deleted.
    pub fn is_tombstone(&self) -> bool {
        self.value.is_none()
    }

    /// Serialize the record to its on-disk byte layout, including the CRC.
    pub fn encode(&self) -> Vec<u8> {
        let (value_size, value_bytes): (u32, &[u8]) = match &self.value {
            Some(value) => (value.len() as u32, value.as_slice()),
            None => (TOMBSTONE_VALUE_SIZE, &[]),
        };
        let key_size = self.key.len() as u32;

        let mut buf = Vec::with_capacity(HEADER_LEN + self.key.len() + value_bytes.len());
        buf.extend_from_slice(&[0u8; 4]); // CRC placeholder, filled in below.
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&key_size.to_le_bytes());
        buf.extend_from_slice(&value_size.to_le_bytes());
        buf.extend_from_slice(&self.key);
        buf.extend_from_slice(value_bytes);

        let crc = crc32(&buf[4..]);
        buf[0..4].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    /// Parse a record from a buffer that begins with one full encoded record.
    ///
    /// `bytes` must be at least [`RecordHeader::record_len`] long. Returns
    /// [`Error::CrcMismatch`] when the stored checksum does not match the
    /// payload, which is how corrupt or torn records are detected.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let header = RecordHeader::decode(bytes)?;
        let total = header.record_len();
        if bytes.len() < total {
            return Err(Error::CorruptRecord(format!(
                "record needs {total} bytes, got {}",
                bytes.len()
            )));
        }

        let actual = crc32(&bytes[4..total]);
        if actual != header.crc {
            return Err(Error::CrcMismatch {
                expected: header.crc,
                actual,
            });
        }

        let key_start = HEADER_LEN;
        let key_end = key_start + header.key_size as usize;
        let key = bytes[key_start..key_end].to_vec();
        let value = if header.is_tombstone() {
            None
        } else {
            Some(bytes[key_end..key_end + header.value_size as usize].to_vec())
        };

        Ok(Self {
            timestamp: header.timestamp,
            key,
            value,
        })
    }
}

/// CRC-32 (ISO-HDLC / zlib polynomial `0xEDB88320`), used to detect corrupt
/// or torn records on disk.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut crc = i as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    0xEDB8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
            *slot = crc;
        }
        table
    });

    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = table[index] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vectors() {
        // Canonical CRC-32/ISO-HDLC check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn put_record_round_trips() {
        let record = Record::put(42, b"key".to_vec(), b"value".to_vec());
        let encoded = record.encode();
        assert_eq!(encoded.len(), HEADER_LEN + 3 + 5);
        assert_eq!(Record::decode(&encoded).unwrap(), record);
    }

    #[test]
    fn tombstone_record_round_trips() {
        let record = Record::tombstone(7, b"gone".to_vec());
        let encoded = record.encode();
        assert_eq!(encoded.len(), HEADER_LEN + 4);
        let decoded = Record::decode(&encoded).unwrap();
        assert_eq!(decoded, record);
        assert!(decoded.is_tombstone());
    }

    #[test]
    fn empty_key_and_value_round_trip() {
        let record = Record::put(1, Vec::new(), Vec::new());
        assert_eq!(Record::decode(&record.encode()).unwrap(), record);
    }

    #[test]
    fn corrupt_payload_is_detected() {
        let mut encoded = Record::put(1, b"k".to_vec(), b"v".to_vec()).encode();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;
        assert!(matches!(
            Record::decode(&encoded),
            Err(Error::CrcMismatch { .. })
        ));
    }

    #[test]
    fn header_decode_rejects_short_input() {
        assert!(matches!(
            RecordHeader::decode(&[0u8; HEADER_LEN - 1]),
            Err(Error::CorruptRecord(_))
        ));
    }
}
