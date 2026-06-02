# Benchmarks

Benchmarking is not wired up yet. The plan is to measure the storage engine with
repeatable workloads before comparing it with other Rust key-value stores.

The benchmark notes should record:

- hardware and operating system
- Rust version
- value sizes
- key distribution
- operation mix
- durability mode
- warm-cache or cold-cache state

Targets for comparison:

- `bitcask-rs`
- `sled`
- `rocksdb`
