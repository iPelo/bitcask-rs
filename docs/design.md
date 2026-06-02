# Design

This store follows the Bitcask shape: writes append to disk, reads use an
in-memory index, and compaction removes obsolete records later.

## Storage Model

`Db` owns two main pieces of state:

- `Log`, which manages numbered data files and record reads/writes
- `KeyDir`, a `HashMap<Vec<u8>, RecordPos>` that points each live key at its
  latest record

Writes never update old records in place. `put` appends a new value record.
`delete` appends a tombstone. The key directory is updated after the append
returns.

Reads do one hash lookup and one positioned file read. There is no full scan on
the read path.

## Record Format

Each record starts with a 20-byte little-endian header:

```text
crc32 | timestamp | key_size | value_size | key | value
 4 B  |    8 B    |   4 B    |    4 B     | var |  var
```

The checksum covers every byte after the checksum field. A delete is encoded as
a record whose value size is `u32::MAX`; no value bytes follow. Empty values are
stored normally with value size `0`.

## Files

Data files are named with six-digit ids:

```text
000001.data
000002.data
000003.data
```

The highest numbered data file is the active file. Older files are immutable.
When the active file would exceed `Options::max_data_file_size`, the log rotates
to the next file id.

## Startup

On open, the log discovers all `*.data` files and rebuilds the key directory in
file-id order. For immutable files, a matching `*.hint` file can provide the
record positions without reading full record values. The active file is always
scanned directly so recovery can catch a torn tail.

If the active file ends with a partial header, partial body, or CRC mismatch,
the file is truncated back to the last valid record.

## Hint Files

Hint files are cache files. A hint must match the current data-file length. If
it is missing, stale, or malformed, startup falls back to scanning the data file
and writes a fresh hint afterward.

Hints store:

- key bytes
- tombstone flag
- record offset
- record size
- record timestamp

## Compaction

`Db::compact()` scans immutable files and rewrites records that are still the
latest version for their key. Stale overwritten records and tombstones are left
behind. After rewritten data is synced, compacted input files and their hints are
removed.

Compaction can be called synchronously through `Db::compact()`. The crate also
provides a background scheduler that periodically checks `Db::stale_ratio()` and
runs compaction when the configured threshold is reached. The scheduler shares a
database through `Arc<Mutex<Db>>`; the public database handle itself is still a
single mutable handle.

Crash-mid-compaction recovery is covered by a simulated test that restores
compacted input files after the rewrite phase. On reopen, later data files win,
so rewritten live values remain visible even if old files are still present.

## TCP Interface

The server binary exposes basic `put`, `get`, and `delete` commands over a small
newline-delimited protocol. Keys and values are hex-encoded on the wire so the
protocol can carry arbitrary bytes while remaining easy to inspect.

The current client is intentionally small. It opens one TCP connection per
command and prints either `OK`, `NOT_FOUND`, or the returned value.

## Current Limits

- no shared concurrent `Db` handle
- no authentication, TLS, or graceful shutdown for the server
- benchmark files are placeholders
