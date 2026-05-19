//! Public API for the Bitcask-style key-value store.
//!
//! A [`Db`] is an append-only log of records plus an in-memory index
//! ([`keydir::KeyDir`]) mapping each key to the position of its latest record.
//! Writes append; reads index-lookup then seek; deletes append a tombstone.

pub mod compaction;
pub mod error;
pub mod keydir;
pub mod log;
pub mod record;
pub mod server;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use error::{Error, Result};

use keydir::KeyDir;
use log::Log;
use record::Record;

/// Configuration used when opening a database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Directory that contains data files, hint files, and lock files.
    pub path: PathBuf,
    /// Whether each write should be synced to durable storage before returning.
    pub sync_on_write: bool,
    /// Maximum size of the active data file before rotation.
    pub max_data_file_size: u64,
}

impl Options {
    /// Create options for a database directory.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            sync_on_write: false,
            max_data_file_size: 64 * 1024 * 1024,
        }
    }
}

/// Database handle.
#[derive(Debug)]
pub struct Db {
    options: Options,
    log: Log,
    keydir: KeyDir,
}

impl Db {
    /// Open or create a database at `path` using default options.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(Options::new(path.as_ref()))
    }

    /// Open or create a database using explicit options.
    ///
    /// The data directory is created if it does not exist, and the in-memory
    /// index is rebuilt by scanning the active data file.
    pub fn open_with_options(options: Options) -> Result<Self> {
        fs::create_dir_all(&options.path)?;
        let log = Log::open(&options.path, options.sync_on_write)?;

        let mut keydir = KeyDir::new();
        for (record, position) in log.scan()? {
            if record.is_tombstone() {
                keydir.remove(&record.key);
            } else {
                keydir.insert(record.key, position);
            }
        }

        Ok(Self {
            options,
            log,
            keydir,
        })
    }

    /// Append a value for `key` and update the in-memory index.
    pub fn put(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<()> {
        let record = Record::put(now_millis(), key.as_ref(), value.as_ref());
        let position = self.log.append(&record)?;
        self.keydir.insert(record.key, position);
        Ok(())
    }

    /// Fetch the latest value for `key`, or `None` if it is absent or deleted.
    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>> {
        let position = match self.keydir.get(key.as_ref()) {
            Some(position) => position,
            None => return Ok(None),
        };
        let record = self.log.read_record(position)?;
        Ok(record.value)
    }

    /// Append a tombstone for `key` and remove it from the in-memory index.
    pub fn delete(&mut self, key: impl AsRef<[u8]>) -> Result<()> {
        let key = key.as_ref();
        let record = Record::tombstone(now_millis(), key);
        self.log.append(&record)?;
        self.keydir.remove(key);
        Ok(())
    }

    /// Number of live keys currently indexed.
    pub fn len(&self) -> usize {
        self.keydir.len()
    }

    /// Whether the database currently has no live keys.
    pub fn is_empty(&self) -> bool {
        self.keydir.is_empty()
    }

    /// Return the options used by this database handle.
    pub fn options(&self) -> &Options {
        &self.options
    }
}

/// Milliseconds since the Unix epoch, stamped onto each record.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
