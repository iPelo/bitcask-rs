//! Property tests for the single-file engine.
//!
//! Each test runs against a fresh database in its own directory under
//! `target/` and compares behaviour to a plain `HashMap` reference model.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use bitcask_rs::Db;
use proptest::prelude::*;

/// A key/value mutation applied to both the database and the reference model.
#[derive(Clone, Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}

/// A small key space so overwrites and deletes of live keys actually happen.
fn key_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0u8..4, 1..3)
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (key_strategy(), prop::collection::vec(any::<u8>(), 0..16))
            .prop_map(|(key, value)| Op::Put(key, value)),
        key_strategy().prop_map(Op::Delete),
    ]
}

/// Every key the strategies above can ever produce — used to probe `get`.
fn all_possible_keys() -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    for a in 0u8..4 {
        keys.push(vec![a]);
        for b in 0u8..4 {
            keys.push(vec![a, b]);
        }
    }
    keys
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty database in a unique directory.
fn fresh_db() -> Db {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("target/test-data/property-{id}");
    let _ = std::fs::remove_dir_all(&path);
    Db::open(&path).expect("open database")
}

proptest! {
    /// After any sequence of puts and deletes, `get` agrees with a plain
    /// `HashMap` model driven by the same operations.
    #[test]
    fn get_matches_a_hashmap_model(ops in prop::collection::vec(op_strategy(), 0..64)) {
        let mut db = fresh_db();
        let mut model: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

        for op in ops {
            match op {
                Op::Put(key, value) => {
                    db.put(&key, &value).expect("put");
                    model.insert(key, value);
                }
                Op::Delete(key) => {
                    db.delete(&key).expect("delete");
                    model.remove(&key);
                }
            }
        }

        for key in all_possible_keys() {
            prop_assert_eq!(db.get(&key).expect("get"), model.get(&key).cloned());
        }
        prop_assert_eq!(db.len(), model.len());
    }

    /// `put(k, v)` immediately followed by `get(k)` returns exactly `v`.
    #[test]
    fn put_then_get_returns_latest_value(
        key in key_strategy(),
        value in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut db = fresh_db();
        db.put(&key, &value).expect("put");
        prop_assert_eq!(db.get(&key).expect("get"), Some(value));
    }

    /// `delete(k)` followed by `get(k)` returns `None`.
    #[test]
    fn delete_then_get_returns_none(
        key in key_strategy(),
        value in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut db = fresh_db();
        db.put(&key, &value).expect("put");
        db.delete(&key).expect("delete");
        prop_assert_eq!(db.get(&key).expect("get"), None);
    }
}
