# bitcask-rs — A Persistent Key-Value Store in Rust

> A from-scratch Bitcask-style key-value store: write-ahead log, in-memory index, crash recovery, compaction, network protocol. Built to learn storage engines and Rust deeply, with honest benchmarks against `sled` and `rocksdb`.

**Status:** 🚧 In development
**Benchmarks:** _coming_
**Tech:** Rust · Tokio · bincode · criterion · proptest

---

## The Problem

Modern databases feel like magic until you build one. This project re-implements the core of a real storage engine so you can speak about *why* databases do what they do — fsync trade-offs, log-structured storage, recovery, compaction, MVCC vs. locking — from first-hand experience instead of textbook chapters.

Reference: the [Bitcask paper](https://riak.com/assets/bitcask-intro.pdf), six pages, very readable. Bitcask powers Riak and is intentionally simple: **append-only data files + in-memory hash index + periodic compaction**.

## What it does

- `put(key, value)` — append record, update in-memory index, optionally fsync
- `get(key)` — index lookup → file seek + read
- `delete(key)` — write a tombstone
- **Crash recovery** — rebuild index from data files on startup; detect torn writes via CRC
- **Compaction** — merge old immutable files, drop overwritten and tombstoned entries
- **Network server** — Tokio TCP, length-prefixed binary protocol
- **CLI client** — speak the protocol

## Why Rust

Rust forces ownership, error handling, and concurrency to be explicit. You'll learn more than the equivalent Go or Python project because the compiler doesn't let you cheat. This is also what makes it differentiating on a CV competing with Python-only candidates.

## Architecture

```
┌────────────────────────────────────────────┐
│             TCP Server (Tokio)             │
└────────────────────┬───────────────────────┘
                     │
             ┌───────▼────────┐
             │  Storage API   │  put / get / delete / scan
             └───────┬────────┘
                     │
       ┌─────────────┴─────────────┐
       │                           │
┌──────▼────────┐         ┌────────▼─────────┐
│   KeyDir      │         │   Data files     │
│ HashMap<Key,  │         │ (append-only)    │
│   {file_id,   │         │                  │
│    offset,    │         │ 000001.data      │
│    size,      │         │ 000002.data      │
│    ts}>       │         │ active.data  ◄─┐ │
└───────────────┘         └──────────────────┘
                                          │
                              ┌───────────▼──────┐
                              │   Compactor      │
                              │  (background)    │
                              └──────────────────┘
```

## On-disk record format

```
┌───────┬───────────┬──────────┬────────────┬──────┬────────┐
│ CRC32 │ Timestamp │ Key size │ Value size │ Key  │ Value  │
│ 4 B   │   8 B     │   4 B    │    4 B     │ var  │  var   │
└───────┴───────────┴──────────┴────────────┴──────┴────────┘
```

Tombstone = special value-size sentinel (e.g. `u32::MAX`).

## Roadmap

### Phase 1 — Rust foundations + design (Week 1)
- [ ] Re-read Rust basics: ownership, lifetimes, traits, `Result`/`?`, error types with `thiserror`
- [ ] Sketch the public API in `lib.rs`: `Db`, `Options`, `put`, `get`, `delete`, `open`
- [ ] Decide serialization: `bincode` (easy) vs hand-rolled (more control)
- [ ] Commit `docs/design.md` describing the layout above

### Phase 2 — Single-file engine (Week 2)
- [ ] Open file, append serialized records
- [ ] Build `HashMap<Vec<u8>, RecordPos>` in memory
- [ ] `get` via file seek + read + deserialize
- [ ] Tombstones for `delete`
- [ ] **Property tests** with `proptest`: `put(k, v)` then `get(k) == Some(v)`; `delete(k)` then `get(k) == None`

### Phase 3 — Crash recovery (Week 3)
- [ ] On startup, scan file and rebuild `KeyDir`
- [ ] CRC validation; detect torn write at tail and truncate
- [ ] Test: spawn child process, kill mid-write, reopen and verify
- [ ] **`docs/durability.md`:** state precisely what you guarantee and what you don't (sync mode, fsync per write vs batched)

### Phase 4 — Multi-file + compaction (Week 4)
- [ ] Rotate to new active file at size threshold
- [ ] Background compactor: merge old immutable files
- [ ] Hint files (compact index of merged files) for faster startup
- [ ] Stress test: 1M writes, kill mid-compaction, verify no data loss

### Phase 5 — Concurrency (Week 5)
- [ ] Start with `RwLock<KeyDir>` — multiple readers, single writer
- [ ] Optional stretch: sharded keydir for parallelism, or full MVCC
- [ ] Concurrency tests with `loom` or `shuttle`

### Phase 6 — Network layer (Week 6)
- [ ] Tokio TCP server
- [ ] Length-prefixed binary protocol (look at RESP for inspiration)
- [ ] CLI client `bitcask-cli` that speaks the same protocol
- [ ] Integration tests via the network, not just in-process

### Phase 7 — Benchmarks (Week 7)
- [ ] `criterion.rs` for micro-benchmarks (put, get, mixed)
- [ ] Throughput test on your M4 Pro: ops/sec at various value sizes
- [ ] Compare against `sled` and `rocksdb` Rust bindings on the same workload
- [ ] **`docs/benchmarks.md`:** report where you win, where you lose, and *why*. Honesty here is more impressive than fake numbers.

### Phase 8 — Polish (Week 8)
- [ ] `cargo doc` clean (every public item documented)
- [ ] Architecture diagram in the README (ASCII or SVG)
- [ ] Blog post: "Building a KV store from scratch in Rust — what I learned"
- [ ] Short demo: server + multi-client CLI session

## Target Project Structure

```
bitcask-rs/
├── README.md
├── Cargo.toml
├── benches/
│   └── kv_bench.rs
├── src/
│   ├── lib.rs                # public API
│   ├── error.rs
│   ├── record.rs             # on-disk record format + CRC
│   ├── keydir.rs             # in-memory index
│   ├── log.rs                # append-only files, rotation
│   ├── compaction.rs
│   ├── server/
│   │   ├── mod.rs
│   │   └── protocol.rs
│   └── bin/
│       ├── server.rs
│       └── client.rs
├── tests/
│   ├── integration.rs
│   ├── recovery.rs
│   └── property.rs
└── docs/
    ├── design.md
    ├── durability.md
    └── benchmarks.md
```

## What I'm trying to learn / demonstrate

- Systems programming in Rust (ownership, errors, async with Tokio)
- Storage engine internals — logs, indices, recovery, compaction
- Concurrency + durability trade-offs articulated clearly
- Honest performance measurement and comparison

## Alternative ideas (if Bitcask doesn't grab you)

- **Mini-Raft** — implement Raft consensus on a 3-5 node cluster
- **Lox interpreter** — follow *Crafting Interpreters*, build tree-walk + bytecode VM
- **Toy search engine** — inverted index + BM25 over Wikipedia dump
- **CRDT collaborative editor** — Yjs-style CRDTs + WebSocket server + tiny web client

Whichever you pick, the same depth-of-engineering bar applies: **tests, benchmarks, documentation, honesty about limitations**.

## Resources

- *Designing Data-Intensive Applications* by Martin Kleppmann (Ch. 3 especially)
- Bitcask paper (linked above)
- *Crafting Interpreters* by Robert Nystrom — free online
- Jon Gjengset, "Crust of Rust" YouTube series
- Tokio Async Book

## License

MIT
