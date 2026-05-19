use bitcask_rs::Db;

/// Open a database in a clean directory under `target/` (which is gitignored).
fn fresh_db(name: &str) -> Db {
    let path = format!("target/test-data/integration-{name}");
    let _ = std::fs::remove_dir_all(&path);
    Db::open(&path).expect("open database")
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
