//! Public API for the Bitcask-style key-value store.

pub mod compaction;
pub mod error;
pub mod keydir;
pub mod log;
pub mod record;
pub mod server;

use std::path::{Path, PathBuf};

pub use error::{Error, Result};

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
}

impl Db {
    /// Open or create a database at `path` using default options.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(Options::new(path.as_ref()))
    }

    /// Open or create a database using explicit options.
    pub fn open_with_options(options: Options) -> Result<Self> {
        Ok(Self { options })
    }

    /// Append a value for `key` and update the in-memory index.
    pub fn put(&mut self, _key: impl AsRef<[u8]>, _value: impl AsRef<[u8]>) -> Result<()> {
        Err(Error::not_implemented("Db::put"))
    }

    /// Fetch the latest value for `key`.
    pub fn get(&self, _key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>> {
        Err(Error::not_implemented("Db::get"))
    }

    /// Append a tombstone for `key` and remove it from the in-memory index.
    pub fn delete(&mut self, _key: impl AsRef<[u8]>) -> Result<()> {
        Err(Error::not_implemented("Db::delete"))
    }

    /// Return the options used by this database handle.
    pub fn options(&self) -> &Options {
        &self.options
    }
}

