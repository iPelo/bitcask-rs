//! Append-only log over the active data file.
//!
//! Phase 2 keeps exactly one data file. Reads and writes use positioned I/O
//! (`pread`/`pwrite` via [`std::os::unix::fs::FileExt`]) so the file cursor is
//! never touched — appends go to a tracked offset and reads seek directly.
//! Rotation across multiple files is introduced in phase 4.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::keydir::RecordPos;
use crate::record::{Record, RecordHeader, HEADER_LEN};

/// Identifier for a data file in the append-only log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DataFileId(pub u64);

impl DataFileId {
    /// File name this id maps to inside the data directory (e.g. `000001.data`).
    pub fn file_name(self) -> String {
        format!("{:06}.data", self.0)
    }
}

/// Append-only log state for a single active data file.
#[derive(Debug)]
pub struct Log {
    directory: PathBuf,
    active_file_id: DataFileId,
    file: File,
    /// Offset at which the next record will be appended (current file length).
    write_offset: u64,
    sync_on_write: bool,
}

impl Log {
    /// Open the active data file in `directory`, creating it if needed.
    pub fn open(directory: impl AsRef<Path>, sync_on_write: bool) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        let active_file_id = DataFileId(1);
        let path = directory.join(active_file_id.file_name());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;
        let write_offset = file.metadata()?.len();
        Ok(Self {
            directory,
            active_file_id,
            file,
            write_offset,
            sync_on_write,
        })
    }

    /// Directory backing this log.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Id of the file currently receiving appends.
    pub fn active_file_id(&self) -> DataFileId {
        self.active_file_id
    }

    /// Total bytes written to the active file.
    pub fn size(&self) -> u64 {
        self.write_offset
    }

    /// Append `record` to the active file and return its on-disk position.
    pub fn append(&mut self, record: &Record) -> Result<RecordPos> {
        let bytes = record.encode();
        let offset = self.write_offset;
        self.file.write_all_at(&bytes, offset)?;
        if self.sync_on_write {
            self.file.sync_data()?;
        }
        self.write_offset += bytes.len() as u64;
        Ok(RecordPos {
            file_id: self.active_file_id.0,
            offset,
            size: bytes.len() as u64,
            timestamp: record.timestamp,
        })
    }

    /// Read and decode the record stored at `pos`.
    pub fn read_record(&self, pos: &RecordPos) -> Result<Record> {
        let mut buf = vec![0u8; pos.size as usize];
        self.file.read_exact_at(&mut buf, pos.offset)?;
        Record::decode(&buf)
    }

    /// Scan the active file from the start, returning every record with its
    /// position. Used to rebuild the [`KeyDir`](crate::keydir::KeyDir) on open.
    pub fn scan(&self) -> Result<Vec<(Record, RecordPos)>> {
        let mut records = Vec::new();
        let mut offset = 0u64;
        while offset < self.write_offset {
            let mut header_buf = [0u8; HEADER_LEN];
            self.file.read_exact_at(&mut header_buf, offset)?;
            let header = RecordHeader::decode(&header_buf)?;
            let size = header.record_len();

            let mut record_buf = vec![0u8; size];
            self.file.read_exact_at(&mut record_buf, offset)?;
            let record = Record::decode(&record_buf)?;

            records.push((
                record,
                RecordPos {
                    file_id: self.active_file_id.0,
                    offset,
                    size: size as u64,
                    timestamp: header.timestamp,
                },
            ));
            offset += size as u64;
        }
        Ok(records)
    }
}
