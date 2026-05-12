# bitcask-rs

A from-scratch Bitcask-style persistent key-value store in Rust.

The longer project brief is in `README-04-systems-rust.md`. This repository is
structured so RustRover can open it directly from `Cargo.toml`.

## Layout

```text
src/
  lib.rs              Public storage API.
  error.rs            Shared error and Result types.
  record.rs           On-disk record format.
  keydir.rs           In-memory key index.
  log.rs              Append-only data files.
  compaction.rs       Compaction planning and execution.
  server/             Network protocol and server modules.
  bin/                CLI entry points.
tests/                Integration, recovery, and property tests.
benches/              Benchmark harnesses.
workloads/            Generated benchmark workloads.
docs/                 Design, durability, and benchmark notes.
scripts/              Local developer utilities.
```

## Benchmark Workloads

Generated benchmark data should go under `workloads/generated/`. That directory
is ignored by git by default so large local datasets do not accidentally enter
the repository.

Keep small, hand-written sample workloads under `workloads/samples/` when they
are useful for tests, examples, or documentation.

## RustRover

Open this repository as a Cargo project from the workspace root. RustRover will
use `Cargo.toml` as the project model. Local JetBrains project settings are
ignored through `.gitignore`.

