# Roadmap

This project is a small Unix-style JSON/JSONL reconnaissance tool for humans and
agents. Its core job is to report where structure and data live, especially when
the caller does not yet know the shape of the input.

## Positioning

`jscan` reports structure. `jq` and `jaq` transform values.

That boundary matters. The tool should locate paths, records, lines, types,
shapes, samples, and evidence. It should not grow into a general projection,
calculation, or reshaping language. When users need transformation, the answer
should be to pipe the located evidence into `jq` or another transformer.

The current product read is narrower and stronger than "faster jq": `jscan` is
a schema-skeleton and query-planning helper for unknown or ugly JSON evidence.
It can recover the mechanical schema contract in one bounded pass. It should not
claim to be a semantic schema author, domain mapper, or investigation reasoning
engine.

## Current State

Implemented:

- Rust CLI scaffold with `paths` and `shape`
- Stable JSON reports with schema tags
- `partial`, `error_count`, and `errors_truncated` metadata
- Per-source read/parse error accounting
- Explicit `--input-format auto|json|jsonl`
- Line-by-line JSONL streaming for explicit JSONL and `.jsonl`/`.ndjson`
- `--strict` for non-zero exit on partial scans
- Bounded scalar sample previews with source line metadata
- Directory scans default to JSON-like files, with `--all-files` escape hatch
- Linear shape inference using a parent-child path index
- `profile --budget <size> --json` with container/record-root detection,
  rooted next-tool hints, token-aware field signals, and emitted-byte budget
  accounting. Rooted hints now render top-level arrays as `.[]` and avoid
  non-selective scalar presence predicates when every candidate field appears
  on every record. Dominant top-level array fields with non-identifier keys use
  jq-safe root bracket syntax like `.["items-list"][]`.
- Repo-local CLI benchmark harness in [bench/run.mjs](bench/run.mjs), with
  deterministic synthetic fixtures and CSV/Markdown output under
  `target/bench-results`. Benchmark rows include semantic answer validation for
  tasks with known answers, so exit-zero/no-output cases are not reported as
  successful measurements.
- Repo-local workflow trial harness in [bench/trials.mjs](bench/trials.mjs),
  comparing known-query `jq`, blind jq probes, `jscan profile` plus final query,
  and raw `rg` where meaningful.
- Private Downloads reconnaissance signal: file-level `profile` can turn opaque
  timestamp/GUID JSON filenames into useful schema skeletons, especially raw
  Splunk `{preview,result}` JSONL exports. Directory-level profile is too mixed,
  suggesting a future `catalog` / `classify` command that groups files by
  inferred schema kind.
- Known Splunk/ZPA control signal: `profile` recovered record root, wrapper
  format, dominant fields, field types, and scalar/array drift from messy
  exports, but did not replace curated domain mapping. This supports the
  schema-skeleton/query-planning wedge and reinforces the semantic boundary.
- Regression tests for malformed input, directory scans, truncation, samples,
  strict mode, stdin-style JSONL, array shapes, auto JSONL fallback, profile
  budgeting, token-aware keywords, and rooted next-tool hints

Verification:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Durable Gates

These gates should be handled in order:

1. **Research before name.** Resolve the Scout vs. Engine vs. hybrid wedge in
   [RESEARCH.md](RESEARCH.md) before choosing a final name.
2. **Name before contract.** The crate/binary name must be settled before
   publishing schema identifiers, docs, package metadata, or agent guidance.
   `jscan` is already taken on crates.io, so this is a Phase 0 decision.
3. **Contract before `grep`.** Existing `paths` and `shape` output contracts
   should be documented before adding more surface area.
4. **Locator before extractor.** `grep` must locate matching evidence first.
   Matching value output may exist, but the tool should not grow into a general
   transformation language.
5. **Benchmarks before optimization.** Performance work should be driven by
   measured workloads, not crate swaps.
6. **Memory bounds before SIMD.** Large-input behavior and bounded memory matter
   more than speculative parser acceleration.

## Phase -1: Product Research

- Fill in [RESEARCH.md](RESEARCH.md).
- Incorporate the private overlay report: initial signal favors
  Scout/Locator-first with a bounded `profile` command.
- Incorporate the initial private benchmark report: `rg`, `jq`/`jaq`, and
  `jg` each win different tasks, so `profile` should recommend the right next
  tool instead of claiming universal speed.
- Pick 8 to 10 seed tasks.
- Write best-known competitor commands for each task.
- Classify each task as Scout, Engine, Both, or Not ours.
- Treat the first wedge as agent-friendly reconnaissance: a bounded
  schema-skeleton/query-planning pass before query delivery.
- Decide whether `jsonq` fits the chosen wedge.

## Phase 0: Finish Foundation

- Resolve the published name:
  - binary name
  - crate name
  - JSON schema identifiers
  - docs terminology
- Add a `--max-depth` guard to avoid stack overflow on pathological nesting.
- Document the execution contract:
  - stdout is report data
  - stderr is diagnostics
  - default exit behavior vs. `--strict`
  - schema versioning policy
  - what `partial` means
  - current memory model and known buffering cases
- Decide whether pretty-mode parse errors remain in stdout or move to stderr.

## Phase 1: Agent Contract For `paths` And `shape`

- Define `profile --budget <size> --json` as the bounded context command that
  combines enough path, shape, sample, and source-layout information for an
  agent to write the next query.
- Publish JSON Schemas for:
  - `paths` report
  - `shape` report
  - `profile` report, if `profile` is added before `grep`
  - shared source/error/path/sample objects
- Add examples for unknown JSON inspection:
  - one file
  - JSONL logs
  - directory of mixed files
  - noisy/malformed input
- Add a short `llms.txt` or agent cheat sheet:
  - run `paths` before writing ad hoc parsing code
  - run `shape` to understand records and optional fields
  - use `--json` for machine consumption
  - use `--strict` when parse completeness matters
- State non-goals clearly:
  - no value transformation
  - no jq-compatible language
  - no TUI-first workflow

## Phase 2: `grep` As A Locator

`grep` should answer: where is the matching evidence?

Start with a JSON query AST before adding a human DSL. Structural predicates are
the primary surface:

- path match
- type match
- key exists / key missing
- object has fields
- array contains an element matching a predicate

Value predicates are secondary conveniences:

- field equals
- string contains
- string regex
- numeric comparison

Output should be evidence-oriented:

- source
- record
- line
- path / pointer template
- value preview
- optional parent/context preview
- reason or explanation when requested

Required options:

- `--limit`
- `--context none|value|parent`
- `--json`
- NDJSON match stream option for large result sets
- `--explain` for query validation and match reasoning

## Phase 2A: `catalog` / `classify` For Mixed Directories

Private Downloads testing showed that directory-level `profile` gets too mixed
when a folder contains unrelated JSON evidence. A catalog command should answer:

> What kinds of JSON files are in this folder, and which files share the same
> schema shape?

Likely command:

```sh
jscan catalog ~/Downloads --json
```

Primary output:

- inferred schema kind, such as:
  - `splunk_preview_result_jsonl`
  - `paged_list_wrapper`
  - `root_array_records`
  - `har_capture`
  - `terraform_state_or_plan`
  - `unknown_json`
- file count
- total parsed records
- representative files
- dominant record root
- top fields / skeleton fields
- parse-error count
- suggested first-pass tool

This should reuse cheap per-file profile/source summaries rather than emitting
one huge blended profile. It is probably more valuable than broadening `profile`
for directories.

## Phase 3: Benchmark Harness

Benchmark the home turf first:

- path inventory
- shape/schema discovery
- large JSONL streaming
- many small files
- noisy/malformed directories
- bounded samples

Search comparisons are secondary:

- exact field search
- path/type search
- nested object predicate search

Compare against:

- `jq`
- `jaq`
- `jsongrep`
- `jsont` / `jt`
- `quicktype`
- `genson-cli`
- `json-to-schema`
- `schemax-cli`
- `drivel`
- `gron`
- `fastgron`
- `duckdb`

Capture:

- wall time
- peak memory
- output size
- exit behavior
- parse-error behavior
- output contract, such as plain paths, path/type/count report, schema, flattened
  assignments, bounded profile, or final query answer

First harness pass exists in [bench/README.md](bench/README.md). It currently
captures wall time, output size, status, command string, output hash, and
semantic label. It also validates expected answers for count-style tasks. Peak
memory capture is still future work.

Before using benchmark results to make a product decision, split rows by output
contract. The latest expanded benchmark showed useful overlap, but `jt fields`
vs. `jscan paths --json`, schema inference vs. bounded profile, and flattening
vs. bounded context are not clean speed comparisons.

Workflow trials now exist too. They are synthetic and should not be treated as
final product evidence, but they provide a stable shape for downstream private
trials: correct answer, total calls, total wall time, and total stdout bytes for
complete investigative workflows.

## Phase 4: Performance And Large Inputs

Measure first, then optimize.

- Add Criterion microbenchmarks for scanner and shape inference.
- Stream large top-level arrays where practical.
- Reduce hot path allocation, especially path key cloning.
- Add parallel file scanning.
- Improve auto-mode sniffing without large buffering.
- Consider `simd-json` only after bottlenecks are measured.

## Phase 5: Input Features

- gzip input
- zstd input
- richer extension detection
- explicit glob/filter controls
- optional all-file noisy scan mode improvements

## Phase 6: UX Polish

- shell completions
- man page
- compact JSON output option
- improved pretty tables
- clarify whether a separate `sample` command adds enough beyond `--samples`
- skip `doctor` unless a real diagnostic need appears

## Phase 7: Packaging

- GitHub Actions
- release builds
- install docs
- Homebrew tap or equivalent if demand exists
- crates.io package if name is available or renamed

## Next Action After Reboot

Start here:

1. Open the downstream profile findings in [RESEARCH.md](RESEARCH.md).
2. Rebuild the branch and have the private-data agent re-run:

   ```sh
   target/release/jscan profile <input> --budget 20kb --json
   ```

3. Confirm the prior review issues are resolved:
   - `.json` NDJSON fallback reports `format: jsonl`
   - actual emitted JSON stays within the 20 KB budget on representative files
   - samples and common values are still present when possible
   - `description` / `apiProtectionEnabled` are not tagged as `keyword:ip`
   - next-tool hints use `$.result`, `$.list[]`, or `$[]`
   - top-level array commands render `.[]`, not `[]`
   - dominant top-level array fields render `.["key"][]`, not `["key"][]`
   - presence `select(...)` hints are only emitted for narrower observed paths
4. Use that feedback to decide whether to:
   - improve `profile`
   - add focused grep
   - improve benchmarks
   - pause/rethink
5. Then return to [RESEARCH.md](RESEARCH.md) and [PROFILE.md](PROFILE.md) as
   needed.
