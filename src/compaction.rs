use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
/// `Arc<Mutex<Db>>` so it can share ownership with the application.
/// Dropping the `CompactionScheduler` (or calling [`stop`]) signals the
/// thread to exit and waits for it to finish cleanly.
///
/// [`stop`]: CompactionScheduler::stop
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::{Arc, Mutex};
/// use bitcask_rs::Db;
/// use bitcask_rs::compaction::{CompactionScheduler, SchedulerOptions};
///
/// let db = Arc::new(Mutex::new(Db::open("my-store").unwrap()));
/// let _sched = CompactionScheduler::start(
///     Arc::clone(&db),
///     SchedulerOptions::default(),
/// );
///
/// // Use the database normally through the Arc<Mutex<Db>>.
/// db.lock().unwrap().put("k", "v").unwrap();
/// // The scheduler stops when `_sched` is dropped.
/// ```
pub struct CompactionScheduler {
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CompactionScheduler {
    /// Spawn the background compaction thread.
    pub fn start(db: Arc<Mutex<Db>>, options: SchedulerOptions) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let thread = thread::spawn(move || {
            loop {
                thread::sleep(options.check_interval);

                if shutdown_flag.load(Ordering::Relaxed) {
                    break;
                }

                let mut guard = match db.lock() {
                    Ok(g) => g,
                    Err(_) => break, // mutex poisoned — give up
                };

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
        self.shutdown.store(true, Ordering::Relaxed);
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
