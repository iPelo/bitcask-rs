use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::log::DataFileId;
use crate::Db;

/// Files selected for a compaction pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompactionPlan {
    pub input_files: Vec<DataFileId>,
}

impl CompactionPlan {
    pub fn new(input_files: Vec<DataFileId>) -> Self {
        Self { input_files }
    }

    pub fn is_empty(&self) -> bool {
        self.input_files.is_empty()
    }
}

/// Tuning knobs for the background compaction scheduler.
#[derive(Clone, Debug)]
pub struct SchedulerOptions {
    /// How often the background thread wakes up to check whether compaction
    /// should run.
    pub check_interval: Duration,
    /// Fraction of total on-disk records that must be stale before a
    /// compaction pass is triggered.  `0.3` means "compact when at least
    /// 30 % of records are stale".
    pub stale_ratio_threshold: f64,
}

impl Default for SchedulerOptions {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            stale_ratio_threshold: 0.3,
        }
    }
}

/// A background thread that periodically calls [`Db::compact`] when the
/// database has accumulated enough stale records.
///
/// The scheduler requires the database to be wrapped in an
/// `Arc<RwLock<Db>>` so it can share ownership with the application.
/// It checks the stale ratio under a read lock and only takes the write
/// lock when a compaction pass is due, so idle checks never block readers.
/// Reads and writes still wait while a compaction pass runs.
/// Dropping the `CompactionScheduler` (or calling [`stop`]) signals the
/// thread to exit and waits for it to finish cleanly.
///
/// [`stop`]: CompactionScheduler::stop
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::{Arc, RwLock};
/// use bitcask_rs::Db;
/// use bitcask_rs::compaction::{CompactionScheduler, SchedulerOptions};
///
/// let db = Arc::new(RwLock::new(Db::open("my-store").unwrap()));
/// let _sched = CompactionScheduler::start(
///     Arc::clone(&db),
///     SchedulerOptions::default(),
/// );
///
/// // Writes take the write lock; reads can share the read lock.
/// db.write().unwrap().put("k", "v").unwrap();
/// assert_eq!(db.read().unwrap().get("k").unwrap(), Some(b"v".to_vec()));
/// // The scheduler stops when `_sched` is dropped.
/// ```
pub struct CompactionScheduler {
    shutdown: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl CompactionScheduler {
    /// Spawn the background compaction thread.
    pub fn start(db: Arc<RwLock<Db>>, options: SchedulerOptions) -> Self {
        let (shutdown, receiver) = mpsc::channel();

        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = receiver.recv_timeout(options.check_interval)
            {
                let due = match db.read() {
                    Ok(guard) => guard.should_compact(options.stale_ratio_threshold),
                    Err(_) => break, // lock poisoned — give up
                };
                if !due {
                    continue;
                }

                let mut guard = match db.write() {
                    Ok(guard) => guard,
                    Err(_) => break, // lock poisoned — give up
                };
                // Re-check: another writer may have compacted between locks.
                if guard.should_compact(options.stale_ratio_threshold) {
                    // Ignore individual compaction errors; the thread keeps
                    // running and will retry on the next interval.
                    let _ = guard.compact();
                }
            }
        });

        Self {
            shutdown,
            thread: Some(thread),
        }
    }

    /// Signal the background thread to stop and block until it exits.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for CompactionScheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}
