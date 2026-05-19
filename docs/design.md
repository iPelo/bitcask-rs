# Design

`bitcask-rs` is a Bitcask-style key-value store: an append-only log of records
plus an in-memory index. This document covers the layout and the decisions made
through Phase 2 (single-file engine). Multi-file storage, compaction, recovery
hardening, concurrency, and the network layer are deferred to later phases.

## Model

- **Writes** append an immutable record to the active data file.
- The **`KeyDir`** holds, in memory, the location of the *latest* record for
  each key.
- **Reads** look the key up in `KeyDir`, then seek directly to that record.
- **Deletes** append a tombstone record and drop the key from `KeyDir`.
- **Startup** rebuilds `KeyDir` by scanning the data file front to back.

```
        put / delete                         get
             │                                │
             ▼                                ▼
      ┌─────────────┐   position        ┌─────────────┐
      │     Log     │◄──────────────────│   KeyDir    │
      │ append-only │                   │ key → pos   │
      │  data file  │── record bytes ──►│ (in memory) │
      └─────────────┘                   └─────────────┘
```

## On-disk record format

Every record is a fixed 20-byte header followed by the key and value bytes.
Integers are little-endian.

```
┌───────┬───────────┬──────────┬────────────┬──────┬────────┐
│ CRC32 │ Timestamp │ Key size │ Value size │ Key  │ Value  │
│ 4 B   │   8 B     │   4 B    │    4 B     │ var  │  var   │
└───────┴───────────┴──────────┴────────────┴──────┴────────┘
```

- **CRC32** covers every byte after itself (timestamp, sizes, key, value). It is
  CRC-32/ISO-HDLC, the same polynomial as zlib/PNG.
- **Timestamp** is milliseconds since the Unix epoch.
- **Tombstone**: `value size == u32::MAX` (`TOMBSTONE_VALUE_SIZE`) and no value
  bytes are written. `RecordHeader::is_tombstone()` is the only place this check
  lives. The largest storable value is therefore `u32::MAX - 1` bytes.
- An empty value (`value size == 0`) is a real, retrievable value and is
  distinct from a tombstone.

## Components

- **`record`** — the format above, plus encode/decode and the CRC. `Record` is
  the logical form (`value: Option<Vec<u8>>`, `None` ⇒ tombstone); `encode`
  produces the byte layout and `decode` validates the CRC.
- **`keydir`** — `KeyDir`, a `HashMap<Vec<u8>, RecordPos>`. `RecordPos` is
  `{ file_id, offset, size, timestamp }`: enough to read one record back.
- **`log`** — `Log` owns the active data file (`000001.data`). It appends
  records, reads a record at a position, and scans the whole file to rebuild
  the index. Reads and writes use positioned I/O (`pread`/`pwrite`), so the file
  cursor is never moved and `get` only needs `&self`.
- **`lib` (`Db`)** — orchestrates the above: `open`, `put`, `get`, `delete`.

## Decisions

### Serialization: hand-rolled (not `bincode`)

The record format is a fixed binary layout that must be byte-stable for
recovery and forward compatibility. Hand-rolling `encode`/`decode` keeps that
layout explicit and lets the CRC cover exactly the bytes we choose, with no
dependency and no hidden framing. `bincode` would add a dependency and its own
framing for no benefit here.

### Errors: hand-rolled enum (not `thiserror`)

`Error` has three variants (`Io`, `CorruptRecord`, `CrcMismatch`). A hand-written
`Display`/`Error`/`From<io::Error>` is short and keeps the crate
dependency-free. `thiserror` can be adopted later if the enum grows.

### Positioned I/O

`Log` uses `FileExt::read_exact_at` / `write_all_at` (Unix `pread`/`pwrite`).
This keeps `get` non-mutating and avoids seek/cursor races, at the cost of being
Unix-only for now.

## What is implemented (Phases 1–2)

- Open/create a database directory and a single active data file.
- `put` / `get` / `delete` against the log + `KeyDir`.
- Index rebuild by scanning the data file on open (clean shutdowns).

## Not yet implemented

- **Crash recovery (Phase 3)** — the scan currently treats any malformed record
  as a hard error. Detecting a torn write at the tail and truncating to the last
  valid record, plus the `fsync` durability story, is Phase 3. See
  `durability.md`.
- **Multi-file + compaction (Phase 4)** — only `000001.data` exists; there is no
  rotation at `max_data_file_size` and no compaction yet.
- **Concurrency (Phase 5)**, **network server (Phase 6)**, **benchmarks
  (Phase 7)**.

See `../README-04-systems-rust.md` for the full phase roadmap.
