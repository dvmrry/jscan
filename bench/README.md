# Benchmark Harness

This harness measures `jscan` against the tools that commonly show up in the
target workflow: `jq`, `jaq`, `rg`, and `jg` / `jsongrep`.

It is intentionally CLI-level rather than a Rust microbenchmark. The current
product question is not "which function is hot?" but:

- how long does a useful reconnaissance pass take?
- how many bytes does it emit?
- when is a raw text tool faster but less structurally safe?
- which competitor tools are available on the machine running the benchmark?

Run:

```sh
node bench/run.mjs
```

Useful options:

```sh
node bench/run.mjs --runs 5 --warmups 1
node bench/run.mjs --no-build --runs 20
node bench/run.mjs --regen
```

Environment overrides:

```sh
JSCAN_BIN=/path/to/jscan node bench/run.mjs
JSCAN_BENCH_RUNS=20 node bench/run.mjs
JSCAN_BENCH_DATA=/tmp/jscan-bench-data node bench/run.mjs
JSCAN_BENCH_OUT=/tmp/jscan-bench-results node bench/run.mjs
```

Outputs:

- deterministic synthetic fixtures under `target/bench-data`
- timestamped CSV and Markdown under `target/bench-results`
- `target/bench-results/latest.csv`
- `target/bench-results/latest.md`

Missing competitor tools are recorded as `status=missing` rather than failing
the run. That lets local and downstream/private runs use the same harness even
when their installed tools differ.

## Reading Results

Do not interpret raw speed alone as the product answer.

`rg` will often win raw text checks. That is useful signal, but it can count
occurrences or lines rather than matching JSON records. `jq`, `jaq`, and `jg`
are structural baselines for different shapes of work. `jscan profile` is not a
drop-in replacement for their filters; it is a bounded reconnaissance pass that
should reduce blind follow-up probes.

For downstream private data, keep the same columns and add notes for:

- whether the command answered the actual investigation question
- whether the output improved the next Splunk/KQL/Grafana/API query
- how many exploratory commands it replaced
- whether sensitive values had to be redacted
