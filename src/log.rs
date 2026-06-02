//! Append-only log over numbered data files.
//!
//! Reads and writes use positioned I/O (`pread`/`pwrite` via
//! [`std::os::unix::fs::FileExt`]) so the file cursor is never touched.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::compaction::CompactionPlan;
use crate::error::{Error, Result};
use crate::keydir::RecordPos;
use crate::record::{Record, RecordHeader, HEADER_LEN};

const HINT_MAGIC: &[u8; 8] = b"BCHINT1\n";
const HINT_HEADER_LEN: usize = 16;
const HINT_ENTRY_FIXED_LEN: usize = 1 + 4 + 8 + 8 + 8;

/// Identifier for a data file in the append-only log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DataFileId(pub u64);

impl DataFileId {
    /// File name this id maps to inside the data directory (e.g. `000001.data`).
    pub fn file_name(self) -> String {
        format!("{:06}.data", self.0)
    }

    /// Hint file name this id maps to inside the data directory.
    pub fn hint_file_name(self) -> String {
        format!("{:06}.hint", self.0)
    }
}

#[derive(Debug)]
struct DataFile {
    file: File,
    len: u64,
}

/// Compact startup metadata for one record in a data file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogEntry {
    pub key: Vec<u8>,
    pub is_tombstone: bool,
    pub position: RecordPos,
}

/// Append-only log state over immutable data files plus one active file.
#[derive(Debug)]
pub struct Log {
    directory: PathBuf,
    active_file_id: DataFileId,
    files: BTreeMap<DataFileId, DataFile>,
    sync_on_write: bool,
    max_data_file_size: u64,
}

impl Log {
    /// Open all data files in `directory`, creating the first active file if needed.
    pub fn open(
        directory: impl AsRef<Path>,
        sync_on_write: bool,
        max_data_file_size: u64,
    ) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;

        let mut file_ids = discover_data_file_ids(&directory)?;
        if file_ids.is_empty() {
            file_ids.push(DataFileId(1));
        }

        let active_file_id = *file_ids
            .iter()
            .max()
            .expect("file_ids is seeded when empty");
        let mut files = BTreeMap::new();
        for file_id in file_ids {
            files.insert(file_id, open_data_file(&directory, file_id, true)?);
        }

        Ok(Self {
            directory,
            active_file_id,
            files,
            sync_on_write,
            max_data_file_size,
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
        self.active_file().map(|file| file.len).unwrap_or(0)
    }

    /// Append `record` to the active file and return its on-disk position.
    pub fn append(&mut self, record: &Record) -> Result<RecordPos> {
        let bytes = record.encode();
        if self.should_rotate(bytes.len() as u64) {
            self.rotate()?;
        }

        let active_file_id = self.active_file_id;
        let sync_on_write = self.sync_on_write;
        let active_file = self.active_file_mut()?;
        let offset = active_file.len;
        active_file.file.write_all_at(&bytes, offset)?;
        if sync_on_write {
            active_file.file.sync_data()?;
        }
        active_file.len += bytes.len() as u64;

        Ok(RecordPos {
            file_id: active_file_id.0,
            offset,
            size: bytes.len() as u64,
            timestamp: record.timestamp,
        })
    }

    /// Read and decode the record stored at `pos`.
    pub fn read_record(&self, pos: &RecordPos) -> Result<Record> {
        let file_id = DataFileId(pos.file_id);
        let file = self.get_file(file_id)?;
        let mut buf = vec![0u8; pos.size as usize];
        file.file.read_exact_at(&mut buf, pos.offset)?;
        Record::decode(&buf)
    }

    /// Scan all data files in order, truncating a torn tail in the active file.
    pub fn scan(&mut self) -> Result<Vec<LogEntry>> {
        let mut entries = Vec::new();
        for file_id in self.file_ids() {
            let recover_tail = file_id == self.active_file_id;
            entries.extend(self.scan_entries_with_options(file_id, recover_tail)?);
        }
        Ok(entries)
    }

    /// Scan a single data file strictly, returning an error for malformed data.
    pub fn scan_file(&self, file_id: DataFileId) -> Result<Vec<(Record, RecordPos)>> {
        let file = self.get_file(file_id)?;
        scan_open_file(file_id, &file.file, file.len, false).map(|(records, _)| records)
    }

    /// Create a compaction plan for all immutable files.
    pub fn compaction_plan(&self) -> CompactionPlan {
        CompactionPlan::new(self.inactive_file_ids())
    }

    /// Sync all open data files.
    pub fn sync_all_data_files(&self) -> Result<()> {
        for file in self.files.values() {
            file.file.sync_data()?;
        }
        Ok(())
    }

    /// Remove immutable files after their live records have been rewritten.
    pub fn remove_files(&mut self, file_ids: &[DataFileId]) -> Result<()> {
        for &file_id in file_ids {
            if file_id == self.active_file_id {
                continue;
            }
            self.files.remove(&file_id);
            match fs::remove_file(self.path_for(file_id)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            match fs::remove_file(self.hint_path_for(file_id)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn should_rotate(&self, record_size: u64) -> bool {
        let Some(active_file) = self.active_file() else {
            return false;
        };
        self.max_data_file_size > 0
            && active_file.len > 0
            && active_file.len + record_size > self.max_data_file_size
    }

    fn rotate(&mut self) -> Result<()> {
        let mut next_id = DataFileId(self.active_file_id.0 + 1);
        while self.files.contains_key(&next_id) || self.path_for(next_id).exists() {
            next_id = DataFileId(next_id.0 + 1);
        }
        let path = self.path_for(next_id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        self.files.insert(next_id, DataFile { file, len: 0 });
        self.active_file_id = next_id;
        Ok(())
    }

    fn scan_entries_with_options(
        &mut self,
        file_id: DataFileId,
        recover_tail: bool,
    ) -> Result<Vec<LogEntry>> {
        let file_len = self.get_file(file_id)?.len;
        if !recover_tail {
            if let Some(entries) = read_hint_file(&self.hint_path_for(file_id), file_id, file_len)? {
                return Ok(entries);
            }
        }

        let file = self.get_file(file_id)?;
        let (records, truncate_to) = scan_open_file(file_id, &file.file, file_len, recover_tail)?;
        if let Some(offset) = truncate_to {
            self.truncate_file(file_id, offset)?;
        }

        let entries = records
            .into_iter()
            .map(|(record, position)| {
                let is_tombstone = record.is_tombstone();
                LogEntry {
                    key: record.key,
                    is_tombstone,
                    position,
                }
            })
            .collect::<Vec<_>>();
        if !recover_tail {
            let data_len = self.get_file(file_id)?.len;
            write_hint_file(&self.hint_path_for(file_id), data_len, &entries)?;
        }
        Ok(entries)
    }

    fn truncate_file(&mut self, file_id: DataFileId, len: u64) -> Result<()> {
        let sync_on_write = self.sync_on_write;
        let file = self.get_file_mut(file_id)?;
        file.file.set_len(len)?;
        if sync_on_write {
            file.file.sync_data()?;
        }
        file.len = len;
        Ok(())
    }

    fn file_ids(&self) -> Vec<DataFileId> {
        self.files.keys().copied().collect()
    }

    fn inactive_file_ids(&self) -> Vec<DataFileId> {
        self.files
            .keys()
            .copied()
            .filter(|file_id| *file_id != self.active_file_id)
            .collect()
    }

    fn active_file(&self) -> Option<&DataFile> {
        self.files.get(&self.active_file_id)
    }

    fn active_file_mut(&mut self) -> Result<&mut DataFile> {
        let active_file_id = self.active_file_id;
        self.get_file_mut(active_file_id)
    }

    fn get_file(&self, file_id: DataFileId) -> Result<&DataFile> {
        self.files.get(&file_id).ok_or_else(|| {
            Error::CorruptRecord(format!("data file {} is not open", file_id.file_name()))
        })
    }

    fn get_file_mut(&mut self, file_id: DataFileId) -> Result<&mut DataFile> {
        self.files.get_mut(&file_id).ok_or_else(|| {
            Error::CorruptRecord(format!("data file {} is not open", file_id.file_name()))
        })
    }

    fn path_for(&self, file_id: DataFileId) -> PathBuf {
        self.directory.join(file_id.file_name())
    }

    fn hint_path_for(&self, file_id: DataFileId) -> PathBuf {
        self.directory.join(file_id.hint_file_name())
    }
}

fn discover_data_file_ids(directory: &Path) -> Result<Vec<DataFileId>> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if let Some(file_id) = parse_data_file_id(&path) {
            ids.push(file_id);
        }
    }
    ids.sort();
    Ok(ids)
}

fn parse_data_file_id(path: &Path) -> Option<DataFileId> {
    let file_name = path.file_name()?.to_str()?;
    let number = file_name.strip_suffix(".data")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok().map(DataFileId)
}

fn open_data_file(directory: &Path, file_id: DataFileId, create: bool) -> Result<DataFile> {
    let path = directory.join(file_id.file_name());
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .open(path)?;
    let len = file.metadata()?.len();
    Ok(DataFile { file, len })
}

fn read_hint_file(
    path: &Path,
    file_id: DataFileId,
    expected_data_len: u64,
) -> Result<Option<Vec<LogEntry>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() < HINT_HEADER_LEN || &bytes[..HINT_MAGIC.len()] != HINT_MAGIC {
        return Ok(None);
    }

    let data_len = read_u64(&bytes, HINT_MAGIC.len())?;
    if data_len != expected_data_len {
        return Ok(None);
    }

    let mut offset = HINT_HEADER_LEN;
    let mut entries = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < HINT_ENTRY_FIXED_LEN {
            return Ok(None);
        }

        let is_tombstone = match bytes[offset] {
            0 => false,
            1 => true,
            _ => return Ok(None),
        };
        offset += 1;

        let key_len = read_u32(&bytes, offset)? as usize;
        offset += 4;
        let record_offset = read_u64(&bytes, offset)?;
        offset += 8;
        let size = read_u64(&bytes, offset)?;
        offset += 8;
        let timestamp = read_u64(&bytes, offset)?;
        offset += 8;

        if bytes.len() - offset < key_len {
            return Ok(None);
        }
        let key = bytes[offset..offset + key_len].to_vec();
        offset += key_len;

        entries.push(LogEntry {
            key,
            is_tombstone,
            position: RecordPos {
                file_id: file_id.0,
                offset: record_offset,
                size,
                timestamp,
            },
        });
    }

    Ok(Some(entries))
}

fn write_hint_file(path: &Path, data_len: u64, entries: &[LogEntry]) -> Result<()> {
    let tmp_path = path.with_extension("hint.tmp");
    let mut file = File::create(&tmp_path)?;
    file.write_all(HINT_MAGIC)?;
    file.write_all(&data_len.to_le_bytes())?;
    for entry in entries {
        file.write_all(&[u8::from(entry.is_tombstone)])?;
        file.write_all(&(entry.key.len() as u32).to_le_bytes())?;
        file.write_all(&entry.position.offset.to_le_bytes())?;
        file.write_all(&entry.position.size.to_le_bytes())?;
        file.write_all(&entry.position.timestamp.to_le_bytes())?;
        file.write_all(&entry.key)?;
    }
    file.sync_data()?;
    drop(file);
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    if end > bytes.len() {
        return Err(Error::CorruptRecord("hint file ended while reading u32".into()));
    }
    Ok(u32::from_le_bytes(bytes[offset..end].try_into().unwrap()))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    let end = offset + 8;
    if end > bytes.len() {
        return Err(Error::CorruptRecord("hint file ended while reading u64".into()));
    }
    Ok(u64::from_le_bytes(bytes[offset..end].try_into().unwrap()))
}

fn scan_open_file(
    file_id: DataFileId,
    file: &File,
    len: u64,
    recover_tail: bool,
) -> Result<(Vec<(Record, RecordPos)>, Option<u64>)> {
    let mut records = Vec::new();
    let mut offset = 0u64;

    while offset < len {
        let remaining = len - offset;
        if remaining < HEADER_LEN as u64 {
            return handle_scan_error(
                recover_tail,
                records,
                offset,
                Error::CorruptRecord(format!(
                    "{} has a partial record header at offset {offset}",
                    file_id.file_name()
                )),
            );
        }

        let mut header_buf = [0u8; HEADER_LEN];
        if let Err(error) = file.read_exact_at(&mut header_buf, offset) {
            return handle_scan_error(recover_tail, records, offset, error.into());
        }

        let header = match RecordHeader::decode(&header_buf) {
            Ok(header) => header,
            Err(error) => return handle_scan_error(recover_tail, records, offset, error),
        };
        let size = header.record_len();
        let size_u64 = size as u64;
        if size_u64 > remaining {
            return handle_scan_error(
                recover_tail,
                records,
                offset,
                Error::CorruptRecord(format!(
                    "{} has a partial record body at offset {offset}",
                    file_id.file_name()
                )),
            );
        }

        let mut record_buf = vec![0u8; size];
        if let Err(error) = file.read_exact_at(&mut record_buf, offset) {
            return handle_scan_error(recover_tail, records, offset, error.into());
        }

        let record = match Record::decode(&record_buf) {
            Ok(record) => record,
            Err(error) => return handle_scan_error(recover_tail, records, offset, error),
        };

        records.push((
            record,
            RecordPos {
                file_id: file_id.0,
                offset,
                size: size_u64,
                timestamp: header.timestamp,
            },
        ));
        offset += size_u64;
    }

    Ok((records, None))
}

fn handle_scan_error(
    recover_tail: bool,
    records: Vec<(Record, RecordPos)>,
    offset: u64,
    error: Error,
) -> Result<(Vec<(Record, RecordPos)>, Option<u64>)> {
    if recover_tail {
        Ok((records, Some(offset)))
    } else {
        Err(error)
    }
}
