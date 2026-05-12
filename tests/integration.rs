use bitcask_rs::{Db, Error};

#[test]
fn open_database_with_default_options() {
    let db = Db::open("target/test-data/integration").expect("open database");

    assert_eq!(db.options().max_data_file_size, 64 * 1024 * 1024);
}

#[test]
fn put_is_not_implemented_yet() {
    let mut db = Db::open("target/test-data/integration-put").expect("open database");

    assert!(matches!(db.put("key", "value"), Err(Error::NotImplemented(_))));
}

