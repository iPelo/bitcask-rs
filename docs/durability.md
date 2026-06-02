# Durability

The store has two write modes:

- buffered writes, the default
- sync-on-write, enabled with `Options::sync_on_write`

Buffered writes return after the record is written to the operating system. A
process crash should leave valid records recoverable, but a machine crash can
lose recent acknowledged writes if the OS has not flushed them.

Sync-on-write calls `sync_data` on the active data file before acknowledging a
write. This is slower, but it gives stronger protection for the record bytes.

## Recovery

Startup scans data files in order and rebuilds the key directory from the latest
record for each key.

Older files are treated as immutable and must contain valid records. The active
file may end with a torn record. Recovery handles:

- partial header
- partial body
- CRC mismatch

When one of those appears at the active file tail, the file is truncated to the
last valid offset and startup continues.

## Hints

Hint files do not carry durability guarantees. They are rebuilt whenever they
are missing, stale, or malformed. Data files are the source of truth.

## Compaction

Compaction rewrites live records before deleting old files. Open data files are
synced before compacted input files are removed.

Directory metadata is not synced yet. After a whole-machine crash, persistence
of newly created or deleted files still depends on the filesystem and mount
settings.
