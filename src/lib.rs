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
    /// Total number of records appended to the log (including stale ones).
    total_appends: usize,
    /// Records that are no longer the latest for their key and will be
    /// reclaimed by the next compaction pass.
    stale_appends: usize,
}

impl Db {
    /// Open or create a database at `path` using default options.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(Options::new(path.as_ref()))
    }

    /// Open or create a database using explicit options.
    ///
    /// The data directory is created if it does not exist, and the in-memory
    /// index is rebuilt by scanning data files.
    pub fn open_with_options(options: Options) -> Result<Self> {
        fs::create_dir_all(&options.path)?;
        let mut log = Log::open(
            &options.path,
            options.sync_on_write,
            options.max_data_file_size,
        )?;

        let mut keydir = KeyDir::new();
        let mut total_appends = 0usize;
        for entry in log.scan()? {
            total_appends += 1;
            if entry.is_tombstone {
                keydir.remove(&entry.key);
            } else {
                keydir.insert(entry.key, entry.position);
            }
        }
        // Every on-disk record that is not currently the latest for its key
        // is stale and will be reclaimed by compaction.
        let stale_appends = total_appends.saturating_sub(keydir.len());

        Ok(Self {
            options,
            log,
            keydir,
            total_appends,
            stale_appends,
        })
    }

    /// Append a value for `key` and update the in-memory index.
    pub fn put(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<()> {
        let record = Record::put(now_millis(), key.as_ref(), value.as_ref());
        let position = self.log.append(&record)?;
        self.total_appends += 1;
        if self.keydir.insert(record.key, position).is_some() {
            // The previous record for this key is now stale.
            self.stale_appends += 1;
        }
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
        self.total_appends += 1;
        if self.keydir.remove(key).is_some() {
            // The deleted value record is now stale.
            self.stale_appends += 1;
        } else {
            // Tombstone written for a key that does not exist — the tombstone
            // itself is immediately stale.
            self.stale_appends += 1;
        }
        Ok(())
    }

    /// Rewrite live records from immutable data files and remove stale files.
    ///
    /// After compaction the stale-record counters are reset so the background
    /// scheduler does not trigger again immediately.
    pub fn compact(&mut self) -> Result<()> {
        let plan = self.log.compaction_plan();
        if plan.is_empty() {
            return Ok(());
        }

        for file_id in plan.input_files.iter().copied() {
            for (record, position) in self.log.scan_file(file_id)? {
                let Some(current_position) = self.keydir.get(&record.key).cloned() else {
                    continue;
                };
                if current_position != position {
                    continue;
                }

                let new_position = self.log.append(&record)?;
                self.total_appends += 1;
                self.keydir.insert(record.key, new_position);
            }
        }

        self.log.sync_all_data_files()?;
        self.log.remove_files(&plan.input_files)?;

        // Reset stale-record accounting: every key now has exactly one live
        // record on disk, so the ratio is 0.
        self.total_appends = self.keydir.len();
        self.stale_appends = 0;

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

    /// Fraction of on-disk records that are stale (`0.0` – `1.0`).
    ///
    /// Returns `0.0` when no records have been appended yet.
    pub fn stale_ratio(&self) -> f64 {
        if self.total_appends == 0 {
            return 0.0;
        }
        self.stale_appends as f64 / self.total_appends as f64
    }

    /// Return `true` when the stale record ratio meets or exceeds `threshold`.
    ///
    /// This is the same check the [`compaction::CompactionScheduler`] uses
    /// internally, exposed here so callers can decide when to call
    /// [`compact`] manually.
    ///
    /// [`compact`]: Db::compact
    pub fn should_compact(&self, threshold: f64) -> bool {
        self.stale_ratio() >= threshold
    }
}

/// Milliseconds since the Unix epoch, stamped onto each record.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db(name: &str) -> Db {
        let path = format!("target/test-data/lib-{name}");
        let _ = std::fs::remove_dir_all(&path);
        Db::open(&path).expect("open")
    }

    #[test]
    fn stale_ratio_is_zero_on_empty_db() {
        let db = fresh_db("stale-empty");
        assert_eq!(db.stale_ratio(), 0.0);
        assert!(!db.should_compact(0.3));
    }

    #[test]
    fn stale_ratio_is_zero_after_fresh_puts() {
        let mut db = fresh_db("stale-no-overwrites");
        db.put("a", "1").unwrap();
        db.put("b", "2").unwrap();
        assert_eq!(db.stale_ratio(), 0.0);
    }

    #[test]
    fn stale_ratio_increases_on_overwrite() {
        let mut db = fresh_db("stale-overwrite");
        db.put("k", "v1").unwrap(); // total=1 stale=0
        db.put("k", "v2").unwrap(); // total=2 stale=1 → 0.5
        assert_eq!(db.stale_ratio(), 0.5);
        assert!(db.should_compact(0.3));
        assert!(!db.should_compact(0.8));
    }

    #[test]
    fn stale_ratio_increases_on_delete() {
        let mut db = fresh_db("stale-delete");
        db.put("k", "v").unwrap(); // total=1 stale=0
        db.delete("k").unwrap();   // total=2 stale=1 → 0.5
        assert_eq!(db.stale_ratio(), 0.5);
    }

    #[test]
    fn stale_ratio_resets_after_compact() {
        let path = "target/test-data/lib-stale-reset";
        let _ = std::fs::remove_dir_all(path);
        let mut opts = Options::new(path);
        opts.max_data_file_size = 50; // force rotation so compact has files to process
        let mut db = Db::open_with_options(opts).unwrap();

        db.put("a", vec![0u8; 16]).unwrap();
        db.put("a", vec![1u8; 16]).unwrap(); // stale v1
        db.put("b", vec![2u8; 16]).unwrap();

        assert!(db.stale_ratio() > 0.0);
        db.compact().unwrap();
        assert_eq!(db.stale_ratio(), 0.0);
        assert!(!db.should_compact(0.3));
    }
}
