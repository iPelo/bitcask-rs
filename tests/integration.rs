use std::path::Path;

use bitcask_rs::{Db, Options};

/// Open a database in a clean directory under `target/` (which is gitignored).
fn fresh_db(name: &str) -> Db {
    let path = format!("target/test-data/integration-{name}");
    let _ = std::fs::remove_dir_all(&path);
    Db::open(&path).expect("open database")
}

fn fresh_db_with_max_file_size(name: &str, max_data_file_size: u64) -> (String, Db) {
    let path = format!("target/test-data/integration-{name}");
    let _ = std::fs::remove_dir_all(&path);
    let mut options = Options::new(&path);
    options.max_data_file_size = max_data_file_size;
    let db = Db::open_with_options(options).expect("open database");
    (path, db)
}

fn data_file_count(path: impl AsRef<Path>) -> usize {
    file_count_with_extension(path, "data")
}

fn hint_file_count(path: impl AsRef<Path>) -> usize {
    file_count_with_extension(path, "hint")
}

fn file_count_with_extension(path: impl AsRef<Path>, extension: &str) -> usize {
    std::fs::read_dir(path)
        .expect("read database directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|file_extension| file_extension == extension)
        })
        .count()
}

#[test]
fn open_database_with_default_options() {
    let db = fresh_db("defaults");

    assert_eq!(db.options().max_data_file_size, 64 * 1024 * 1024);
    assert!(!db.options().sync_on_write);
    assert!(db.is_empty());
}

#[test]
fn put_then_get_returns_the_value() {
    let mut db = fresh_db("put-get");

    db.put("name", "ada").expect("put");

    assert_eq!(db.get("name").expect("get"), Some(b"ada".to_vec()));
}

#[test]
fn get_missing_key_returns_none() {
    let db = fresh_db("missing");

    assert_eq!(db.get("absent").expect("get"), None);
}

#[test]
fn put_overwrites_the_previous_value() {
    let mut db = fresh_db("overwrite");

    db.put("k", "first").expect("put");
    db.put("k", "second").expect("put");

    assert_eq!(db.get("k").expect("get"), Some(b"second".to_vec()));
    assert_eq!(db.len(), 1);
}

#[test]
fn delete_removes_the_key() {
    let mut db = fresh_db("delete");

    db.put("k", "v").expect("put");
    db.delete("k").expect("delete");

    assert_eq!(db.get("k").expect("get"), None);
    assert!(db.is_empty());
}

#[test]
fn empty_value_is_distinct_from_a_deleted_key() {
    let mut db = fresh_db("empty-value");

    db.put("k", "").expect("put");
    assert_eq!(db.get("k").expect("get"), Some(Vec::new()));

    db.delete("k").expect("delete");
    assert_eq!(db.get("k").expect("get"), None);
}

#[test]
fn reopen_recovers_keys_from_the_log() {
    let path = "target/test-data/integration-reopen";
    let _ = std::fs::remove_dir_all(path);

    {
        let mut db = Db::open(path).expect("open database");
        db.put("alpha", "1").expect("put");
        db.put("beta", "2").expect("put");
        db.put("alpha", "3").expect("put"); // overwrite an existing key
        db.delete("beta").expect("delete");
    }

    let db = Db::open(path).expect("reopen database");

    assert_eq!(db.get("alpha").expect("get"), Some(b"3".to_vec()));
    assert_eq!(db.get("beta").expect("get"), None);
    assert_eq!(db.len(), 1);
}

#[test]
fn writes_rotate_to_new_data_files() {
    let (path, mut db) = fresh_db_with_max_file_size("rotation", 50);

    db.put("a", vec![1; 16]).expect("put");
    db.put("b", vec![2; 16]).expect("put");
    db.put("c", vec![3; 16]).expect("put");

    assert_eq!(data_file_count(&path), 3);
    assert_eq!(db.get("a").expect("get"), Some(vec![1; 16]));
    assert_eq!(db.get("b").expect("get"), Some(vec![2; 16]));
    assert_eq!(db.get("c").expect("get"), Some(vec![3; 16]));

    drop(db);
    let db = Db::open(&path).expect("reopen database");
    assert_eq!(db.get("a").expect("get"), Some(vec![1; 16]));
    assert_eq!(db.get("b").expect("get"), Some(vec![2; 16]));
    assert_eq!(db.get("c").expect("get"), Some(vec![3; 16]));
}

#[test]
fn compaction_rewrites_live_records_and_removes_stale_files() {
    let (path, mut db) = fresh_db_with_max_file_size("compaction", 50);

    db.put("a", vec![1; 16]).expect("put");
    db.put("a", vec![2; 16]).expect("put");
    db.put("b", vec![3; 16]).expect("put");

    let before = data_file_count(&path);
    assert!(before >= 3);

    db.compact().expect("compact");

    let after = data_file_count(&path);
    assert!(after < before);
    assert_eq!(db.get("a").expect("get"), Some(vec![2; 16]));
    assert_eq!(db.get("b").expect("get"), Some(vec![3; 16]));

    drop(db);
    let db = Db::open(&path).expect("reopen database");
    assert_eq!(db.get("a").expect("get"), Some(vec![2; 16]));
    assert_eq!(db.get("b").expect("get"), Some(vec![3; 16]));
    assert_eq!(db.len(), 2);
}

#[test]
fn reopen_creates_hint_files_for_immutable_data_files() {
    let (path, mut db) = fresh_db_with_max_file_size("hints", 50);

    db.put("a", vec![1; 16]).expect("put");
    db.put("b", vec![2; 16]).expect("put");
    db.put("c", vec![3; 16]).expect("put");
    drop(db);

    assert_eq!(hint_file_count(&path), 0);

    let db = Db::open(&path).expect("reopen database");

    assert_eq!(db.get("a").expect("get"), Some(vec![1; 16]));
    assert_eq!(db.get("b").expect("get"), Some(vec![2; 16]));
    assert_eq!(db.get("c").expect("get"), Some(vec![3; 16]));
    assert_eq!(hint_file_count(&path), 2);
}

#[test]
fn compaction_stress_preserves_latest_values() {
    let (path, mut db) = fresh_db_with_max_file_size("compaction-stress", 128);

    for round in 0..50u8 {
        for key_id in 0..20u8 {
            let key = format!("key:{key_id}");
            db.put(key.as_bytes(), vec![round, key_id]).expect("put");
        }
    }
    for key_id in 0..10u8 {
        let key = format!("key:{key_id}");
        db.delete(key.as_bytes()).expect("delete");
    }

    db.compact().expect("compact");

    for key_id in 0..10u8 {
        let key = format!("key:{key_id}");
        assert_eq!(db.get(key.as_bytes()).expect("get"), None);
    }
    for key_id in 10..20u8 {
        let key = format!("key:{key_id}");
        assert_eq!(db.get(key.as_bytes()).expect("get"), Some(vec![49, key_id]));
    }

    drop(db);
    let db = Db::open(&path).expect("reopen database");
    assert_eq!(db.len(), 10);
    for key_id in 10..20u8 {
        let key = format!("key:{key_id}");
        assert_eq!(db.get(key.as_bytes()).expect("get"), Some(vec![49, key_id]));
    }
}
