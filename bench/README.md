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

For full workflow trials:

```sh
node bench/trials.mjs
```

## Nix Shell

For reproducible local competitor testing, enter the flake dev shell:

```sh
nix develop
```

The shell includes the project toolchain plus the main packaged competitors:

- Rust: `cargo`, `rustc`, `rustfmt`, `clippy`
- harness runtime: `node`, `python`, `uv`, `go`
- search/query tools: `jq`, `jaq`, `rg`, `jg`, `jt` / `jsont`
- schema/flatten/table baselines: `quicktype`, `gron`, `fastgron`, `duckdb`
- benchmark helpers: `hyperfine`, GNU `coreutils`

Some newer schema competitors are not directly packaged in nixpkgs yet.
Bootstrap them separately before full comparison runs:

```sh
./bench/bootstrap-competitors.sh
```

Those extras cover `genson-cli`, `json-to-schema`, `schemax-cli`, and `drivel`.

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
- deterministic workflow-trial fixtures under `target/trial-data`
- timestamped workflow-trial CSV and Markdown under `target/trial-results`
- `target/trial-results/latest.csv`
- `target/trial-results/latest.md`

Missing competitor tools are recorded as `status=missing` rather than failing
the run. That lets local and downstream/private runs use the same harness even
when their installed tools differ.

Some tasks also define an expected semantic answer. For example, the synthetic
Splunk fixture has 10,000 `ConnectionStatus` fields and the synthetic ZIA array
has 2,000 `BLOCK` actions. If a command exits zero but emits the wrong answer,
the row is recorded as `status=wrong_answer` instead of `ok`.

For `jg` count-style tasks, the harness uses `--porcelain` so the answer column
receives a bare count instead of human text such as `Found matches: 10000`.

## Reading Results

Do not interpret raw speed alone as the product answer.

`rg` will often win raw text checks. That is useful signal, but it can count
occurrences or lines rather than matching JSON records. `jq`, `jaq`, and `jg`
are structural baselines for different shapes of work. `jscan profile` is not a
drop-in replacement for their filters; it is a bounded reconnaissance pass that
should reduce blind follow-up probes.

Compare rows by output contract before drawing conclusions. For example,
`jt fields` and `jscan paths --json` both discover paths, but `jt fields`
emits a compact field list while `jscan paths` emits a stable JSON report with
path counts, type counts, source/error metadata, and display/path variants.
That is useful product signal, but not a clean implementation-speed comparison.
Add narrower rows when needed, such as a plain path-list mode or a competitor
command that emits comparable path/type/count metadata.

For downstream private data, keep the same columns and add notes for:

- whether the command answered the actual investigation question
- whether the output improved the next Splunk/KQL/Grafana/API query
- how many exploratory commands it replaced
- whether sensitive values had to be redacted

When adding a benchmark task, prefer adding an `expectedAnswer` and `answerFrom`
extractor. Exit status alone is not enough; some tools can succeed while
emitting no useful answer for a given input format.

## Workflow Trials

`bench/trials.mjs` measures complete paths to a correct answer, not just one
command at a time. Each trial compares:

- `oracle_jq`: a known-good final query, as a lower bound.
- `blind_jq_probes`: small exploratory jq probes followed by the final query.
- `profile_then_jq`: `jscan profile --budget 20kb --json` followed by the
  final query.
- `raw_rg`: raw occurrence count where a text baseline is meaningful.

These trials are still synthetic, but they answer a better product question:
whether the profile pass pays for itself by replacing enough discovery work.
The private downstream trial should keep this shape and swap in representative
redacted raw cases.
