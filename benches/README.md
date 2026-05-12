# Benchmarks

This directory is for benchmark harnesses, starting with `kv_bench.rs`.

Planned benchmark dimensions:

- Operation mix: put-heavy, get-heavy, mixed, delete-heavy.
- Key distribution: sequential, uniform random, zipfian/hotset.
- Value sizes: small metadata values through larger payloads.
- Durability mode: buffered writes vs sync-on-write.

Generated benchmark inputs should live in `workloads/generated/`.

