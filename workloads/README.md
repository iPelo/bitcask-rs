# Workloads

Use this directory for benchmark datasets and workload descriptions.

## Structure

```text
workloads/
  generated/       Large generated workloads, ignored by git.
  samples/         Small committed examples useful for docs and tests.
```

Recommended generated workload format:

```text
op,key,value
put,user:1,alice
get,user:1,
delete,user:1,
```

CSV is simple enough to inspect and easy to replace later if binary workloads
are needed for speed.

