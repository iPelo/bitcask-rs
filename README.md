# bitcask-rs

A Bitcask-style persistent key-value store in Rust: an append-only log on disk,
an in-memory hash index, and crash recovery. It ships as a library and as a
small TCP server with a command-line client, using only the standard library.

## Design

Based on the [Bitcask paper](https://riak.com/assets/bitcask-intro.pdf). Every
write appends a record to a log file. An in-memory index points each key at its
latest record, so a read is one hash lookup plus one positioned disk read.

### Record format

```text
┌───────┬───────────┬──────────┬────────────┬──────┬────────┐
│ CRC32 │ Timestamp │ Key size │ Value size │ Key  │ Value  │
│ 4 B   │   8 B     │   4 B    │    4 B     │ var  │  var   │
└───────┴───────────┴──────────┴────────────┴──────┴────────┘
```

- Integers are little-endian. The CRC-32 covers every byte after itself.
- A delete is a tombstone: value size `u32::MAX` and no value bytes. Keys and
  values must be shorter than `u32::MAX` bytes; larger ones are rejected with
  `Error::TooLarge`.
- Records live in numbered data files (`000001.data`, `000002.data`, …). Only
  the highest-numbered file is active. When the next record would push it past
  `max_data_file_size` (default 64 MiB), a new file is started and the old one
  becomes immutable.

### Keydir

The keydir is a `HashMap` from key bytes to `(file id, offset, size,
timestamp)` for the latest record of each key. `put` appends and updates the
map, `delete` appends a tombstone and removes the key, and `get` reads the
record with `pread` and checks its CRC before returning the value. All keys
must fit in memory; values stay on disk.

### Hint files

On open, each immutable data file without a usable hint is scanned once and a
`NNNNNN.hint` file is written next to it. The hint holds the data file's length
and a compact list of `(key, offset, size, timestamp, tombstone)` entries, with
no values. On later opens the index is rebuilt from hints instead of reading
every value. A hint is only used if its recorded length matches the data file.
Hints carry no checksum, but every record read still checks the record's CRC.

### Compaction

`Db::compact` rewrites the live records from every immutable file into the
active file (which may rotate), syncs the data files, and then deletes the
input files and their hints. Stale records in the active file stay until it
becomes immutable.

The rewritten records always land in higher-numbered files than the inputs.
If the process dies after rewriting but before deleting, recovery still replays
files in order and the newest copy wins. Tombstones are dropped during
compaction: a deleted key's earlier records are in the same or older files, so
they are removed in the same pass.

`CompactionScheduler` runs compaction on a background thread. Every 30 seconds
(configurable) it checks the stale ratio, `(records on disk − live keys) /
records on disk`, and compacts when the ratio reaches the threshold (default
0.3). The database lock is held while compaction runs.

### Recovery

Opening a database replays every data file in id order, using hints where
possible, to rebuild the keydir. In the active file, the scan stops at the first
invalid record (a partial header, a partial body, or a CRC mismatch). The file
is then truncated there, which drops a write torn by a crash and anything after
it. Corruption in an immutable file is never silently skipped. It fails the
open when the file is scanned, or fails the `get` with a CRC error when the
file's hint was used instead.

## Benchmarks

Measured with `cargo bench` (criterion, median of the run) on a MacBook Pro
(Apple M4 Pro, 14 cores, 48 GB RAM, internal APFS SSD), macOS 27.0.1,
rustc 1.100.0, on AC power.

| Benchmark | Workload | Median time | Throughput |
| --- | --- | --- | --- |
| `put` | 16 B key, 100 B value, 10k distinct keys, `sync_on_write` off | 2.56 µs | ~390k ops/s |
| `get` | hit, 10k keys indexed, data likely in the OS page cache | 623 ns | ~1.6M ops/s |
| `compact` | 10k live keys, 40k records (75% stale), 1 MiB files | 156.5 ms | ~256k records/s |

What these numbers do and do not measure:

- `put` runs with the default `sync_on_write = false`, so each write lands
  in the OS page cache and is not flushed to the SSD. With `sync_on_write`
  on, every put waits for a sync and is much slower.
- `get` reads from a ~1.4 MB data file written just before the run. That
  file is almost certainly still in the OS page cache, so this measures the
  index lookup, a cached `pread` and the CRC check, not SSD read latency.
- `compact` includes syncing the rewritten data files before the old files
  are deleted.

Numbers vary by machine and filesystem; rerun `cargo bench` to compare.

## Usage

Requires stable Rust 1.85 or newer. Runs on Linux and macOS.

```bash
git clone https://github.com/iPelo/bitcask-rs.git
cd bitcask-rs
cargo test
cargo bench
```

### Library

```rust
use bitcask_rs::{Db, Options};

let mut options = Options::new("my-store");
options.sync_on_write = true; // fsync each record (slower)
let mut db = Db::open_with_options(options)?;

db.put("name", "ada")?;
assert_eq!(db.get("name")?, Some(b"ada".to_vec()));
db.delete("name")?;
assert_eq!(db.get("name")?, None);

if db.should_compact(0.3) {
    db.compact()?;
}
```

`Db::open(path)` uses the default options. To compact automatically, wrap the
database in `Arc<RwLock<Db>>` and start a
`compaction::CompactionScheduler`.

### Server and client

```bash
cargo run --bin bitcask-server -- --path data --addr 127.0.0.1:4000 --compact-threshold 0.3
```

In another terminal:

```bash
cargo run --bin bitcask-cli -- put name ada    # OK
cargo run --bin bitcask-cli -- get name        # ada
cargo run --bin bitcask-cli -- delete name     # OK
cargo run --bin bitcask-cli -- get name        # NOT_FOUND
```

The server runs one thread per connection and compacts in the background when
the stale ratio reaches `--compact-threshold` (default 0.3). The wire protocol
is one line per request, with keys and values hex-encoded:

```text
PUT <hex-key> <hex-value>   ->  OK
GET <hex-key>               ->  VALUE <hex-value> | NOT_FOUND
DELETE <hex-key>            ->  OK
(any error)                 ->  ERROR <message>
```

A request line longer than 1 MiB gets `ERROR request too large` and the
connection is closed.

### Tests

`cargo test` runs unit tests, integration tests, crash-recovery tests (torn
writes, CRC corruption, interrupted compaction) and `proptest` model tests that
compare the store against a `HashMap` across puts, deletes, compaction and
reopening.

## Limitations

This is a learning project, not production storage:

- The server has no authentication or TLS.
- Use one database handle per directory; there is no lock file.
- Writes are buffered by the OS by default. `sync_on_write` syncs record
  bytes, but directory entries are not synced, so file creation and deletion
  may not survive power loss.

## License

MIT. See [LICENSE](LICENSE).
