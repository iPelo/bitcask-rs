# bitcask-rs

A persistent key-value store written in Rust, based on Bitcask. It saves data
in append-only files and provides both a library and a simple TCP server with
a command-line client.

## Features

- Store, retrieve, overwrite, and delete keys and values.
- Recover saved data when the database reopens, including incomplete writes.
- Rotate data files and compact old records to reclaim space.
- Run compaction manually or with a background scheduler.
- Access the store through a TCP server that handles multiple clients.

## Technologies

- Rust and its standard library for storage, networking, and threads.
- Cargo for building and running the project.
- `proptest` for randomized tests alongside unit and integration tests.

## Getting Started

Install stable Rust and Cargo. The project runs on Linux and macOS.

```bash
git clone https://github.com/iPelo/bitcask-rs.git
cd bitcask-rs
cargo test
cargo run --bin bitcask-server -- --path data --addr 127.0.0.1:4000
```

In another terminal, from the same directory:

```bash
cargo run --bin bitcask-cli -- put name ada    # OK
cargo run --bin bitcask-cli -- get name        # ada
cargo run --bin bitcask-cli -- delete name     # OK
cargo run --bin bitcask-cli -- get name        # NOT_FOUND
```

Data stays in the `data/` directory between runs. The library exposes
`Db::open`, `put`, `get`, `delete`, and `compact` for use in Rust programs.

This is a local learning project: the server has no authentication or TLS.
Use one database handle per directory. Writes are buffered by default;
`Options::sync_on_write` syncs record bytes but does not guarantee recovery
from power loss.

## Benchmarks

Measured with `cargo bench` (criterion, median of the run) on a MacBook Pro
(Apple M4 Pro, 14 cores, 48 GB RAM, internal APFS SSD), macOS 27.0.1,
rustc 1.100.0, on AC power. Default options: writes are not synced per call.

| Benchmark | Workload | Median time | Throughput |
| --- | --- | --- | --- |
| `put` | 16 B key, 100 B value, 10k distinct keys | 2.56 µs | ~390k ops/s |
| `get` | hit, 10k keys indexed | 623 ns | ~1.6M ops/s |
| `compact` | 10k live keys, 40k records (75% stale), 1 MiB files | 156.5 ms | ~256k records/s |

Numbers vary by machine and filesystem; rerun `cargo bench` to compare.
