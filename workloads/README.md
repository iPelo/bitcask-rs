# Workloads

This directory holds benchmark workload inputs.

```text
workloads/
  generated/       large generated workloads, ignored by git
  samples/         small committed examples
```

The sample CSV format is:

```text
op,key,value
put,user:1,alice
get,user:1,
delete,user:1,
```

CSV is easy to inspect while the benchmark tooling is still small. A binary
format can replace it later if input parsing becomes part of the measurement.
