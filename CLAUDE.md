# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Status

**This repository is public.** Never commit secrets, private keys, local tokens, personal machine paths, large generated datasets, or IDE workspace state. The `.gitignore` already excludes `.idea/`, `.env*`, key/cert files (`*.pem`, `*.key`, `*.p12`, `*.pfx`), runtime DB files (`*.data`, `*.hint`, `*.lock`, `/data/`, `/db/`, `/store/`), and `/workloads/generated/**` — preserve these guards.

`bitcask-rs` is in an early scaffold state. Most public methods on `Db` currently return `Error::NotImplemented`. The integration test in `tests/integration.rs` asserts that behavior. When implementing a phase, update or remove those `NotImplemented` assertions accordingly.

The full project brief and phase roadmap lives in `README-04-systems-rust.md`. `AGENTS.md` contains additional handoff notes on data/git safety.

## Common Commands

```bash
cargo check                 # type-check without producing binaries
cargo build                 # build library + both binaries
cargo build --release       # optimized build for benchmarks
cargo fmt                   # format (rustfmt component is pinned in rust-toolchain.toml)
cargo clippy --all-targets  # lint
cargo test                  # run all tests
cargo test --test integration             # run one integration test file
cargo test put_is_not_implemented_yet     # run a single test by name
cargo test -- --ignored     # run tests gated with #[ignore] (recovery, property)
cargo run --bin bitcask-server
cargo run --bin bitcask-cli
```

Toolchain is pinned to `stable` via `rust-toolchain.toml` with `rustfmt` and `clippy` components. Minimum Rust version is 1.75 (`Cargo.toml`).

## Architecture

Bitcask design: append-only data files + in-memory hash index + periodic compaction. The crate is organized into single-responsibility modules — keep them that way.

- `src/lib.rs` — public surface: `Db`, `Options`, `Error`, `Result`. `Db::open` and `Db::open_with_options` are the entry points; `put`/`get`/`delete` are the I/O surface.
- `src/record.rs` — on-disk record format. Header is `CRC32(4) + Timestamp(8) + KeySize(4) + ValueSize(4)` = `HEADER_LEN`. Tombstones use `value_size == u32::MAX` (`TOMBSTONE_VALUE_SIZE`); `RecordHeader::is_tombstone()` is the single source of truth — don't reinvent the check.
- `src/keydir.rs` — `KeyDir`: in-memory `HashMap<Vec<u8>, RecordPos>` mapping a key to `{file_id, offset, size, timestamp}`. `get` returns a reference into the map; `insert`/`remove` return the prior position.
- `src/log.rs` — `Log` owns the data directory and the active `DataFileId`. Rotation, append, and read-at-offset belong here.
- `src/compaction.rs` — `CompactionPlan` enumerates input file ids for a compaction pass. Compaction execution will be wired here.
- `src/server/` — network layer. `protocol.rs` defines the wire `Request`/`Response` enums (`Put`/`Get`/`Delete` + `Ok`/`Value`/`Error`). The Tokio TCP server goes alongside.
- `src/bin/server.rs` and `src/bin/client.rs` — `bitcask-server` and `bitcask-cli` binaries, currently placeholders.

Data flow (target): network → `Request` → `Db` method → `KeyDir` lookup or `Log` append (+ optional fsync per `Options::sync_on_write`) → `Response`. Compaction reads old data files, writes live records to a new file, then atomically swaps `KeyDir` entries.

## Tests

- `tests/integration.rs` — exercises the public `Db` API. Uses `target/test-data/…` as the database directory so artifacts land under `target/` (already gitignored).
- `tests/recovery.rs` — Phase 3; currently `#[ignore]`. Plan is to spawn a child process, kill mid-write, reopen, and verify.
- `tests/property.rs` — `proptest`-based round-trip checks; currently `#[ignore]`. Add to `dev-dependencies` when implementing.
- `benches/kv_bench.rs` — Criterion harness placeholder for Phase 7.

When you implement a feature, replace the matching `#[ignore]` placeholder with the real test rather than adding a parallel file.

## Data and Workload Conventions

- Generated benchmark workloads go in `workloads/generated/` (gitignored except `.gitkeep`).
- Small, hand-written, non-sensitive samples go in `workloads/samples/` (committed).
- Local runtime databases go in `/data/`, `/db/`, or `/store/` — all gitignored. Tests that touch the filesystem should write under `target/`.

## Roadmap Awareness

The 8-phase roadmap in `README-04-systems-rust.md` drives the work order: foundations → single-file engine → recovery → multi-file + compaction → concurrency → network → benchmarks → polish. When picking up work, check which phase the requested change belongs to so the rest of the codebase isn't pulled ahead of its phase.
