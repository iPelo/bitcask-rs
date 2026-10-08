use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use bitcask_rs::record::Record;
use bitcask_rs::{Db, Options};

fn fresh_path(name: &str) -> PathBuf {
    let path = PathBuf::from(format!("target/test-data/recovery-{name}"));
    let _ = fs::remove_dir_all(&path);
    path
}

fn append_bytes(path: impl AsRef<Path>, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open data file");
    file.write_all(bytes).expect("append bytes");
    file.sync_all().expect("sync data file");
}

#[test]
fn truncates_partial_record_at_active_file_tail() {
    let path = fresh_path("partial-tail");
    let data_file = path.join("000001.data");

    {
        let mut db = Db::open(&path).expect("open database");
        db.put("good", "value").expect("put");
    }
    let valid_len = fs::metadata(&data_file).expect("metadata").len();

    let torn_record = Record::put(1, b"bad".to_vec(), b"incomplete".to_vec()).encode();
    append_bytes(&data_file, &torn_record[..torn_record.len() - 3]);

    let db = Db::open(&path).expect("recover database");

    assert_eq!(db.get("good").expect("get"), Some(b"value".to_vec()));
    assert_eq!(db.get("bad").expect("get"), None);
    assert_eq!(fs::metadata(&data_file).expect("metadata").len(), valid_len);
}

#[test]
fn truncates_crc_mismatch_at_active_file_tail() {
    let path = fresh_path("crc-tail");
    let data_file = path.join("000001.data");

    {
        let mut db = Db::open(&path).expect("open database");
        db.put("good", "value").expect("put");
    }
    let valid_len = fs::metadata(&data_file).expect("metadata").len();

    let mut corrupt_record = Record::put(1, b"bad".to_vec(), b"corrupt".to_vec()).encode();
    let last = corrupt_record.len() - 1;
    corrupt_record[last] ^= 0xFF;
    append_bytes(&data_file, &corrupt_record);

    let db = Db::open(&path).expect("recover database");

    assert_eq!(db.get("good").expect("get"), Some(b"value".to_vec()));
    assert_eq!(db.get("bad").expect("get"), None);
    assert_eq!(fs::metadata(&data_file).expect("metadata").len(), valid_len);
}

// Restore compaction inputs to simulate an interruption after rewriting
// live values but before deleting the old files. Later file IDs must win.
#[test]
fn recovers_when_input_files_survive_a_crash_mid_compaction() {
    let path = fresh_path("crash-compact");
    let data_dir = &path;

    // Small file cap forces a rotation on every write.
    // Each record is HEADER(20) + key(1) + value(16) = 37 bytes; the second
    // write to the same file would push it past the 50-byte limit.
    let mut opts = Options::new(&path);
    opts.max_data_file_size = 50;

    {
        let mut db = Db::open_with_options(opts.clone()).expect("open");
        db.put("a", vec![1u8; 16]).expect("put a_v1"); // lands in file 1
        db.put("a", vec![2u8; 16]).expect("put a_v2"); // lands in file 2 (file 1 full)
        db.put("b", vec![3u8; 16]).expect("put b_v1"); // lands in file 3 (file 2 full)
    }

    // Snapshot every .data file before compaction runs.
    let mut snapshots: HashMap<String, Vec<u8>> = HashMap::new();
    for entry in fs::read_dir(data_dir).expect("read dir") {
        let p = entry.expect("entry").path();
        if p.extension().and_then(|e| e.to_str()) == Some("data") {
            let name = p.file_name().unwrap().to_str().unwrap().to_owned();
            snapshots.insert(name, fs::read(&p).expect("read snapshot"));
        }
    }

    // Run a normal compaction pass: rewrites live records, removes input files.
    {
        let mut db = Db::open_with_options(opts.clone()).expect("reopen for compact");
        db.compact().expect("compact");
    }

    // Simulate the crash: put the pre-compaction files back on disk.
    // The directory now has the compaction output *plus* the "deleted" inputs —
    // the same state the process would have left behind on a mid-compaction kill.
    for (name, bytes) in &snapshots {
        fs::write(data_dir.join(name), bytes).expect("restore snapshot");
    }

    // Recovery must yield the correct live state.
    let db = Db::open_with_options(opts).expect("recover after simulated crash");
    assert_eq!(
        db.get("a").expect("get a"),
        Some(vec![2u8; 16]),
        "latest value for 'a' must win"
    );
    assert_eq!(
        db.get("b").expect("get b"),
        Some(vec![3u8; 16]),
        "value for 'b' must survive"
    );
    assert_eq!(db.len(), 2, "exactly two live keys");
}
