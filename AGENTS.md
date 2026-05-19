# Agent Handoff

This repository is public. Do not commit secrets, private keys, local tokens,
personal machine paths, large generated datasets, local database files, or IDE
workspace state.

## Current State

`bitcask-rs` is no longer only a skeleton. Phase 2 single-file storage work has
started:

- `Db::open` creates the database directory and rebuilds the in-memory index by
  scanning the active log file.
- `Db::put`, `Db::get`, and `Db::delete` are implemented for a single active
  data file.
- `src/record.rs` implements fixed binary record encoding/decoding with CRC-32.
- `src/log.rs` appends records, reads records by offset, and scans `000001.data`.
- `tests/integration.rs` covers put/get/delete/overwrite/reopen behavior.
- `tests/property.rs` uses `proptest` against a `HashMap` model.

The project brief and full roadmap are in `README-04-systems-rust.md`.
Claude-specific guidance is in `CLAUDE.md`.

## Public Repo Safety

Keep these out of git:

- `.idea/`
- `.claude/`
- `.env`, `.env.*`, `.envrc`
- private keys and cert-like files (`*.pem`, `*.key`, `*.p12`, `*.pfx`)
- runtime database files (`*.data`, `*.hint`, `*.lock`)
- generated workloads under `workloads/generated/`
- local benchmark/profiling outputs

`Cargo.lock` is intentionally allowed even though other `*.lock` files are
ignored. Commit it once generated so builds and benchmarks are reproducible.

Small, synthetic, non-sensitive examples can live in `workloads/samples/`.

## Verification Notes

This shell did not have `cargo` or `rustc` available when checked, so the new
Rust code has been reviewed by inspection but not compiled here. Once the Rust
toolchain is available, run:

```bash
cargo fmt
cargo check
cargo test
```

## Next Useful Work

1. Compile and test the Phase 2 implementation.
2. Fix any compiler or clippy findings.
3. Start Phase 3 crash recovery: tolerate a torn tail record and truncate back
   to the last valid record.

