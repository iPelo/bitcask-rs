# Design

`bitcask-rs` follows the Bitcask model:

- Writes append immutable records to the active data file.
- The latest record location for each key is stored in memory in `KeyDir`.
- Reads use `KeyDir` to seek directly to a record in a data file.
- Deletes are tombstone records.
- Startup recovery rebuilds `KeyDir` by scanning data files.
- Compaction rewrites live records from older files into newer files.

See `README-04-systems-rust.md` for the full project brief.

