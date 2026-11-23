//! Property tests for the storage engine.
//!
//! Each test runs against a fresh database in its own directory under
//! `target/` and compares behaviour to a plain `HashMap` reference model.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use bitcask_rs::{Db, Options};
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

/// An operation for the lifecycle model test: a mutation, or an engine
/// event that must not change the visible contents of the database.
#[derive(Clone, Debug)]
enum LifecycleOp {
    Mutate(Op),
    Compact,
    Reopen,
}

fn lifecycle_op_strategy() -> impl Strategy<Value = LifecycleOp> {
    prop_oneof![
        8 => op_strategy().prop_map(LifecycleOp::Mutate),
        1 => Just(LifecycleOp::Compact),
        1 => Just(LifecycleOp::Reopen),
    ]
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique, empty directory for one test case.
fn fresh_path() -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("target/test-data/property-{id}");
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// A fresh, empty database in a unique directory.
fn fresh_db() -> Db {
    Db::open(fresh_path()).expect("open database")
}

/// Options with a tiny file cap so data files rotate every few records,
/// giving compaction immutable files to rewrite.
fn small_file_options(path: &str) -> Options {
    let mut options = Options::new(path);
    options.max_data_file_size = 64;
    options
}

fn apply(db: &mut Db, model: &mut HashMap<Vec<u8>, Vec<u8>>, op: Op) {
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

proptest! {
    /// After any sequence of puts and deletes, `get` agrees with a plain
    /// `HashMap` model driven by the same operations.
    #[test]
    fn get_matches_a_hashmap_model(ops in prop::collection::vec(op_strategy(), 0..64)) {
        let mut db = fresh_db();
        let mut model: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

        for op in ops {
            apply(&mut db, &mut model, op);
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

proptest! {
    // Compaction and reopen sync files to disk, which is slow on some
    // platforms, so this test runs fewer cases than the default 256.
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Interleaving compaction and reopening with puts and deletes never
    /// changes what `get` returns, checked after every step and once more
    /// after a final reopen.
    #[test]
    fn get_matches_a_hashmap_model_across_compaction_and_reopen(
        ops in prop::collection::vec(lifecycle_op_strategy(), 0..96),
    ) {
        let path = fresh_path();
        let mut db = Db::open_with_options(small_file_options(&path)).expect("open");
        let mut model: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

        for op in ops {
            match op {
                LifecycleOp::Mutate(op) => apply(&mut db, &mut model, op),
                LifecycleOp::Compact => db.compact().expect("compact"),
                LifecycleOp::Reopen => {
                    drop(db);
                    db = Db::open_with_options(small_file_options(&path)).expect("reopen");
                }
            }
            for key in all_possible_keys() {
                prop_assert_eq!(db.get(&key).expect("get"), model.get(&key).cloned());
            }
            prop_assert_eq!(db.len(), model.len());
        }

        drop(db);
        let db = Db::open_with_options(small_file_options(&path)).expect("final reopen");
        for key in all_possible_keys() {
            prop_assert_eq!(db.get(&key).expect("get"), model.get(&key).cloned());
        }
        prop_assert_eq!(db.len(), model.len());
    }
}
