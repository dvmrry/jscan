# Next Implementation Plan

This plan assumes the current product read is correct:

> `jscan` is a bounded reconnaissance and query-planning helper for unknown or
> ugly JSON evidence. It is not a jq replacement, a hot-path query engine, a
> formal schema generator, or a raw-text search tool.

The next work should make that positioning sharper, not broader.

## Goals

1. Help an agent choose the right next tool with fewer blind probes.
2. Make mixed directories useful without producing one over-broad blended
   profile.
3. Keep output bounded, honest, and machine-readable.
4. Avoid spending time on engines where other tools already win.

## Non-Goals For This Pass

- Do not add a jq-compatible expression language.
- Do not chase `rg` for raw text or `fastgron` for flattening.
- Do not build a formal schema generator to compete with `quicktype`,
  `genson-cli`, or schema-specific tools.
- Do not add SIMD or parser swaps unless a benchmark proves parse time is the
  limiting issue for the chosen product lane.
- Do not build caching/reuse until repeated large-file workflows prove it is
  worth the extra surface.

## Proposed Order

### 1. Make `next_tools` Decision-Like

Current problem:

`profile.next_tools` is useful, but still too much like a list of observations.
The next step should make it say things closer to:

- use `rg` for raw literal checks
- use `jscan grep` for bounded structural probes and per-predicate counts
- use `jq` or `jaq` for transformation once the shape is known
- use `fastgron` for flattening
- normalize JSONL before `quicktype`
- use `catalog` for mixed directories

Implementation shape:

- Extend next-tool hints with a clearer task label, such as:
  - `raw_literal`
  - `structural_probe`
  - `transform`
  - `flatten`
  - `schema_generation`
  - `mixed_directory_catalog`
- Keep the existing `tool`, `reason`, `caveat`, and `command` fields.
- Add tests for representative shapes:
  - Splunk JSONL wrapper
  - paged `list` wrapper
  - top-level array records
  - mixed directory
  - opaque `.json` file that is actually JSONL

Acceptance:

- `profile` recommends `jscan grep` only when it has a useful record root or
  observed path.
- `profile` recommends `rg` as raw text only, never as a structural count.
- `profile` recommends schema tools only when the input shape is clean enough
  to make that route plausible.
- Docs clearly say these are next-step recommendations, not authoritative
  answers.

Why first:

This directly addresses the biggest product gap from the inefficiency review:
the tool needs to help choose among existing tools, not pretend to replace them.

### 2. Add `catalog` For Mixed Directories

Current problem:

Directory-level `profile` over mixed downloads or evidence folders gets too
blended. It can still find useful facts, but it forces unrelated schemas into
one report.

Proposed command:

```sh
jscan catalog <dir> --json
```

Primary output:

- schema or container kind
- file count
- parsed record count
- representative files
- dominant record root
- top fields or skeleton fields
- parse-error count
- suggested first-pass command

Likely group keys:

- inferred container kind
- record root display path
- top-level field fingerprint
- dominant array field
- format: JSON, JSONL, fallback JSONL

Acceptance:

- A directory with many small paged-wrapper files is grouped instead of
  returning hundreds of source/container/root rows.
- Opaque Splunk JSONL files named by timestamp or GUID are grouped as Splunk
  result-style JSONL.
- HAR, Terraform, root arrays, paged wrappers, and unknown JSON get distinct
  groups when their shapes differ.
- `catalog` stays a classifier. It does not emit full per-path inventories for
  every group unless explicitly requested later.

Why second:

This was one of the strongest real-world signals from private/Downloads-style
testing. It also avoids making `profile` carry every directory workflow.

### 3. Add Narrow Profile Modes

Current problem:

A 20 KB profile is bounded but still broad. Agents sometimes need a smaller
answer, such as only record roots, only high-value paths, or only query hints.

Possible interface:

```sh
jscan profile <input> --focus overview --json
jscan profile <input> --focus roots --json
jscan profile <input> --focus paths --json
jscan profile <input> --focus schema --json
jscan profile <input> --focus hints --json
```

Initial implementation can be output filtering over the existing collector.
Only optimize collection later if benchmarks show the filtered modes need it.

Acceptance:

- Each focus mode has a documented contract.
- Focus modes reduce output bytes materially on representative fixtures.
- `--focus hints` can answer "what should I run next?" without dumping a full
  profile.
- `--focus roots` can answer "where are records?" for Splunk, paged wrappers,
  root arrays, and dominant array fields.

Why third:

This fixes context bloat without prematurely creating a cache, daemon, or
multi-command workflow engine.

### 4. Unify JSONL Detection And Stream Opaque `.json` JSONL

Current problem:

JSONL detection has improved, but the central real-world case remains important:
Splunk or API exports may be line-delimited JSON while using a `.json` suffix.

Plan:

- Share one content detector across `paths`, `shape`, `profile`, and `grep`.
- Avoid a full whole-file JSON parse when a content sniff is confident that the
  file is line-delimited JSON.
- Preserve current strict/error semantics.
- Add regression tests for:
  - valid `.json` JSONL
  - malformed multiline JSON that must not fall back to line-by-line parsing
  - empty files
  - mixed valid and invalid JSONL lines

Acceptance:

- `profile`, `paths`, and `grep` classify the same input the same way.
- Opaque `.json` JSONL streams without first buffering the entire file.
- Multiline JSON errors remain honest and do not become misleading line errors.

Why fourth:

This is correctness and scale work, but not the highest product gap now that the
current benchmarks are small and parse-bound.

### 5. Revisit Reuse Or Caching Only With Evidence

Current problem:

Repeated commands reparse the same input. That is wasteful for large files, but
adding a cache is a serious surface-area commitment.

Do not implement this yet. First collect evidence from real workflows:

- input size
- number of repeated `jscan` commands per investigation
- whether the same file is reprocessed across turns
- whether a saved report artifact would be acceptable

Possible later direction:

```sh
jscan scan <input> --out report.jscan.json
jscan grep --from report.jscan.json ...
jscan profile --from report.jscan.json ...
```

Risks:

- cached artifacts become another schema contract
- stale cache invalidation becomes user-visible
- it can pull the project toward a database/indexer instead of a Unix tool

Decision gate:

Only build reuse if repeated large-file evidence shows profile/paths/grep are
regularly chained over the same input and reparsing is the dominant cost.

## Verification For Each Step

Every implementation step should include:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `nix flake check`
- one benchmark smoke run through `nix develop`
- focused CLI tests for schema/contract behavior

Benchmark reporting should keep the current categories:

- fair competitor races
- contract or answer mismatch rows
- solo coverage rows
- expected failures
- missing optional tools

Rows should enter "fair competitor races" only when they have the same task,
same output contract, successful status, and same validated non-empty answer.

## Suggested Opus Review Questions

1. Is the ordering right: routing, then catalog, then focus modes, then JSONL
   streaming?
2. Should `catalog` come before decision-like `next_tools`?
3. Is `--focus` the right interface, or should these be separate subcommands?
4. Does extending `next_tools` require a schema bump or can it remain
   backward-compatible?
5. Is there a simpler way to classify mixed directories without inventing too
   much schema machinery?
6. Are we deferring caching for the right reasons?
7. Are any of these steps drifting toward a query engine or schema generator?

## Recommended Next Commit

Start with step 1 only:

> Make `profile.next_tools` decision-like without changing the core scanners.

That is the smallest change that most directly improves the product wedge.
