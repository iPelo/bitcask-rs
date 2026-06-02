use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use bitcask_rs::record::Record;
use bitcask_rs::Db;

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
