# bitcask-rs

`bitcask-rs` is a small Bitcask-style key-value store written in Rust. It uses
append-only data files, an in-memory key directory, checksummed records, startup
recovery, file rotation, hint files, and synchronous compaction.

The project is intentionally compact. The goal is to keep the storage path easy
to inspect while still covering the pieces that make a log-structured store
interesting: recovery, stale data cleanup, and clear durability trade-offs.

## Current Status

Implemented:

- `Db::open`, `put`, `get`, `delete`, and `compact`
- append-only data files named `000001.data`, `000002.data`, and so on
- CRC-checked records with tombstones for deletes
- rebuild of the in-memory index on startup
- truncation of a torn record at the active file tail
- file rotation using `Options::max_data_file_size`
- hint files for immutable data files
- synchronous compaction of immutable files
- stale-record tracking with `Db::stale_ratio` and `Db::should_compact`
- optional background compaction scheduler
- TCP server and CLI client for basic `put`, `get`, and `delete` commands
- integration, recovery, and property tests
- crash-mid-compaction recovery coverage

Still planned:

- concurrency around shared database handles
- real benchmark harnesses and comparison runs
- production-grade server features such as authentication, TLS, and graceful
  shutdown

## Quick Start

```bash
cargo test
```

Use the library API directly:

```rust
use bitcask_rs::Db;

let mut db = Db::open("data")?;
db.put("name", "ada")?;
assert_eq!(db.get("name")?, Some(b"ada".to_vec()));
db.delete("name")?;
assert_eq!(db.get("name")?, None);
# Ok::<(), bitcask_rs::Error>(())
```

Run the TCP server:

```bash
cargo run --bin bitcask-server -- --path data --addr 127.0.0.1:4000
```

Use the CLI from another shell:

```bash
cargo run --bin bitcask-cli -- put name ada
cargo run --bin bitcask-cli -- get name
cargo run --bin bitcask-cli -- delete name
```

## Repository Layout

```text
src/
  lib.rs              Public database API
  error.rs            Error and Result types
  record.rs           Binary record format and CRC checks
  keydir.rs           In-memory key index
  log.rs              Data files, recovery, hints, and rotation
  compaction.rs       Compaction planning and background scheduling
  server/             TCP protocol types and parsing
  bin/                Server and client binaries
tests/                Integration, recovery, and property tests
benches/              Benchmark placeholders
workloads/            Benchmark workload samples
docs/                 Design, durability, and benchmark notes
```

## Data Files

Runtime files are ignored by git:

- `*.data`
- `*.hint`
- `*.lock`
- `/data/`
- `/db/`
- `/store/`

Generated benchmark workloads belong under `workloads/generated/`, which is
also ignored except for `.gitkeep`.
