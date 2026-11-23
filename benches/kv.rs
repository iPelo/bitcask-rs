//! Criterion benchmarks for the core operations: put, get and compact.
//!
//! Run with `cargo bench`. Databases live under `target/bench-data/` and use
//! default options (writes buffered by the OS, not synced per write).

use std::hint::black_box;
use std::path::PathBuf;

use bitcask_rs::{Db, Options};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};

const KEY_COUNT: usize = 10_000;
const VALUE_LEN: usize = 100;

fn bench_path(name: &str) -> PathBuf {
    let path = PathBuf::from(format!("target/bench-data/{name}"));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn key(index: usize) -> [u8; 16] {
    let mut key = *b"key-000000000000";
    key[4..].copy_from_slice(format!("{index:012}").as_bytes());
    key
}

fn put(c: &mut Criterion) {
    let mut db = Db::open(bench_path("put")).expect("open");
    let value = vec![7u8; VALUE_LEN];
    let mut index = 0usize;

    let mut group = c.benchmark_group("put");
    group.throughput(Throughput::Elements(1));
    group.bench_function("16B key, 100B value", |b| {
        b.iter(|| {
            db.put(key(index % KEY_COUNT), &value).expect("put");
            index += 1;
        })
    });
    group.finish();
}

fn get(c: &mut Criterion) {
    let mut db = Db::open(bench_path("get")).expect("open");
    let value = vec![7u8; VALUE_LEN];
    for index in 0..KEY_COUNT {
        db.put(key(index), &value).expect("put");
    }
    let mut index = 0usize;

    let mut group = c.benchmark_group("get");
    group.throughput(Throughput::Elements(1));
    group.bench_function("hit, 10k keys", |b| {
        b.iter(|| {
            let found = db.get(key(index % KEY_COUNT)).expect("get");
            index += 1;
            black_box(found)
        })
    });
    group.finish();
}

/// Each iteration compacts a fresh database holding 10k live keys, each
/// overwritten four times (75% stale), spread over 1 MiB data files.
fn compact(c: &mut Criterion) {
    const ROUNDS: usize = 4;
    let path = bench_path("compact");
    let value = vec![7u8; VALUE_LEN];

    let mut group = c.benchmark_group("compact");
    group.sample_size(10);
    group.throughput(Throughput::Elements((KEY_COUNT * ROUNDS) as u64));
    group.bench_function("10k keys x4 overwrites", |b| {
        b.iter_batched(
            || {
                let _ = std::fs::remove_dir_all(&path);
                let mut options = Options::new(&path);
                options.max_data_file_size = 1024 * 1024;
                let mut db = Db::open_with_options(options).expect("open");
                for _ in 0..ROUNDS {
                    for index in 0..KEY_COUNT {
                        db.put(key(index), &value).expect("put");
                    }
                }
                db
            },
            |mut db| {
                db.compact().expect("compact");
                db
            },
            BatchSize::PerIteration,
        )
    });
    group.finish();
}

criterion_group!(benches, put, get, compact);
criterion_main!(benches);
