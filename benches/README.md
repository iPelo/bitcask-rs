# Benchmarks

Benchmark harnesses belong here. `kv_bench.rs` is a placeholder until the
storage API is ready for measured runs.

Useful dimensions:

- put-heavy, get-heavy, mixed, and delete-heavy workloads
- sequential, uniform random, and hot-key distributions
- small and medium value sizes
- buffered writes versus sync-on-write

Generated inputs should live in `workloads/generated/`.
