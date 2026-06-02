//! Tests for the background compaction scheduler.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bitcask_rs::compaction::{CompactionScheduler, SchedulerOptions};
use bitcask_rs::{Db, Options};

fn fresh_db_small_files(name: &str) -> (String, Db) {
    let path = format!("target/test-data/sched-{name}");
    let _ = std::fs::remove_dir_all(&path);
    let mut opts = Options::new(&path);
    opts.max_data_file_size = 50; // forces rotation on every write
    let db = Db::open_with_options(opts).expect("open");
    (path, db)
}

// ---------------------------------------------------------------------------
// should_compact() / stale_ratio() unit-level checks
// ---------------------------------------------------------------------------

#[test]
fn fresh_db_has_zero_stale_ratio() {
    let (_, db) = fresh_db_small_files("ratio-fresh");
    assert_eq!(db.stale_ratio(), 0.0);
    assert!(!db.should_compact(0.3));
}

#[test]
fn overwrite_raises_stale_ratio_above_threshold() {
    let (_, mut db) = fresh_db_small_files("ratio-overwrite");
    db.put("k", "v1").unwrap(); // total=1 stale=0  ratio=0.0
    db.put("k", "v2").unwrap(); // total=2 stale=1  ratio=0.5
    assert!(
        db.stale_ratio() >= 0.5,
        "ratio should be ≥ 0.5, got {}",
        db.stale_ratio()
    );
    assert!(db.should_compact(0.3));
    assert!(!db.should_compact(0.9));
}

#[test]
fn compact_resets_stale_ratio_to_zero() {
    let (_, mut db) = fresh_db_small_files("ratio-reset");
    db.put("a", vec![0u8; 16]).unwrap();
    db.put("a", vec![1u8; 16]).unwrap(); // stale v1
    db.put("b", vec![2u8; 16]).unwrap();

    assert!(db.stale_ratio() > 0.0);
    db.compact().unwrap();
    assert_eq!(db.stale_ratio(), 0.0);
}

// ---------------------------------------------------------------------------
// Scheduler integration test
// ---------------------------------------------------------------------------

/// Start the scheduler with a very short interval, accumulate enough stale
/// records to exceed the threshold, then verify that compaction has run by
/// the time we stop the scheduler.
///
/// Uses a 20 ms check interval and waits up to 500 ms — that is 25 chances
/// for the scheduler to fire, making the test robust against scheduler jitter.
#[test]
fn scheduler_triggers_compaction_automatically() {
    let path = "target/test-data/sched-auto";
    let _ = std::fs::remove_dir_all(path);
    let mut opts = Options::new(path);
    opts.max_data_file_size = 50;

    let db = Arc::new(Mutex::new(
        Db::open_with_options(opts.clone()).expect("open"),
    ));

    // Write enough overwrites to push the stale ratio well above 0.3.
    {
        let mut guard = db.lock().unwrap();
        guard.put("a", vec![0u8; 16]).unwrap(); // file 1
        guard.put("a", vec![1u8; 16]).unwrap(); // file 2  → a_v1 stale
        guard.put("b", vec![2u8; 16]).unwrap(); // file 3
        guard.put("b", vec![3u8; 16]).unwrap(); // file 4  → b_v1 stale
        assert!(
            guard.should_compact(0.3),
            "pre-condition: ratio must exceed threshold before scheduler starts"
        );
    }

    let scheduler = CompactionScheduler::start(
        Arc::clone(&db),
        SchedulerOptions {
            check_interval: Duration::from_millis(20),
            stale_ratio_threshold: 0.3,
        },
    );

    // Wait up to 500 ms for at least one compaction cycle to complete.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let guard = db.lock().unwrap();
        if !guard.should_compact(0.3) {
            break; // compaction ran and reset the ratio
        }
        if std::time::Instant::now() >= deadline {
            panic!("scheduler did not trigger compaction within 500 ms");
        }
    }

    scheduler.stop();

    // Verify data integrity after background compaction.
    let guard = db.lock().unwrap();
    assert_eq!(guard.get("a").unwrap(), Some(vec![1u8; 16]));
    assert_eq!(guard.get("b").unwrap(), Some(vec![3u8; 16]));
    assert_eq!(guard.len(), 2);
}

/// Dropping the scheduler handle must stop the thread cleanly without a panic.
#[test]
fn dropping_scheduler_stops_thread() {
    let path = "target/test-data/sched-drop";
    let _ = std::fs::remove_dir_all(path);
    let db = Arc::new(Mutex::new(Db::open(path).expect("open")));

    let sched = CompactionScheduler::start(
        Arc::clone(&db),
        SchedulerOptions {
            check_interval: Duration::from_millis(50),
            stale_ratio_threshold: 0.3,
        },
    );

    // Drop immediately — must not hang or panic.
    drop(sched);
}
