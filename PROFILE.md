# Profile Command Plan

`profile` is the first product-wedge command.

It should run one bounded pass over unknown JSON, JSONL, or JSON-like evidence
and return enough context for an agent to stop blind probing with repeated
`jq`/`jaq` commands.

## Purpose

Answer this question:

> What is this data shaped like, where are the useful records, and what should
> an agent know before writing the next query?

The command should be optimized for network/security investigation workflows
over logs, Splunk exports, API responses, saved schemas, and mixed evidence
directories.

Example:

```sh
jscan profile evidence.json --budget 20kb --json
```

The `jscan` binary name and `jscan.profile.v1` schema identifier are
provisional until the project name is settled.

## Inputs

Support the same input sources as `paths` and `shape`:

- stdin
- JSON files
- JSONL / NDJSON files
- directories of JSON-like files
- noisy directories when `--all-files` is used

The profiler should explicitly recognize common container shapes:

- top-level object
- top-level array
- JSONL / NDJSON records
- multiple JSON documents when detectable (deferred; the current input layer
  does not parse concatenated JSON documents)
- Splunk-style row wrappers: `{preview,result}` or `{result}`
- paged API wrappers: `{totalPages,totalCount,list}` or similar
- wrapper objects with a dominant array field

Embedded JSON in string fields is potentially valuable, especially Splunk
`_raw`, but should not be part of the first implementation unless it is cheap
and optional. Treat it as a later feature flag or separate mode.

## Output Budget

`profile` should default to a bounded output budget. The budget bounds output
size, not input memory. In v1, top-level JSON documents inherit the current
whole-document parsing behavior; streaming large top-level arrays is later
performance work.

Initial option:

```sh
--budget 20kb
```

`--budget` is meaningful for `--json` output. v1 should use advisory top-N
limits and deterministic section dropping rather than an exact byte-packing
loop.

Budgeting rules:

- Deterministic output for the same input and options.
- Always preserve high-priority structural metadata.
- Trim lower-priority examples before trimming path/type summaries.
- When trimming occurs, report what was omitted.
- The JSON report should remain valid and schema-stable even when budgeted.

Priority order:

1. report metadata: schema, partial/error counts, source summaries
2. detected input/container shapes
3. record-root guesses
4. top path/type facts
5. optional/required field summaries
6. type variations
7. enum-like/common values
8. bounded samples/examples
9. suggested next commands

## Report Contents

The JSON output should be versioned, for example:

```json
{
  "schema": "jscan.profile.v1",
  "partial": false,
  "error_count": 0,
  "errors_truncated": false,
  "budget": {
    "requested_bytes": 20480,
    "estimated_bytes": 12345,
    "truncated": false,
    "omitted": []
  },
  "sources": [],
  "containers": [],
  "record_roots": [],
  "path_facts": [],
  "shape_facts": [],
  "type_variations": [],
  "common_values": [],
  "samples": [],
  "next_tools": [],
  "next_commands": []
}
```

### Source Summary

For each source:

- source path
- records parsed
- errors
- effective input format
- detected container shape
- likely record root, when applicable

Implementation note: the current `PathReport` aggregates paths across all
sources. Per-source container and record-root detection therefore needs new
scanner-side summary data. v1 should retain a cheap per-source top-level summary
rather than re-scan inputs.

### Container Detection

Examples:

- `root_object`
- `root_array`
- `jsonl_records`
- `splunk_result_wrapper`
- `splunk_preview_result_wrapper`
- `paged_list_wrapper`
- `dominant_array_field`
- `empty`
- `unknown`

### Record Roots

Record-root guesses should identify where meaningful event objects appear:

- root JSON object
- root array item
- JSONL record
- `result` inside Splunk row
- `list[]` inside paged API wrapper
- dominant array field item

Each guess should include:

- display path
- pointer template
- confidence
- reason
- record count, when known

### Path Facts

Summarize observed paths:

- display path
- pointer template
- count
- observed types
- source/record coverage where cheap
- whether the path is under the guessed record root

Prefer high-signal paths:

- frequent paths
- fields with mixed types
- fields likely useful for investigations:
  - time/timestamp
  - user
  - host
  - device
  - action
  - status
  - error
  - policy
  - url/domain/ip
  - source/destination
  - connector/tunnel/app

### Shape Facts

Summarize object fields:

- object path
- field name
- required/optional
- presence percentage
- observed types

Prioritize:

- likely record-root objects
- fields with mixed types
- optional fields with non-trivial presence
- fields likely useful for filtering

### Type Variations

Highlight paths where multiple types were observed:

- path
- type counts
- sample previews if budget allows

### Common Values

Detect enum-like values for low-cardinality scalar fields:

- string and boolean fields first
- small integer fields if low-cardinality
- include counts when cheap; v1 may emit candidate values without counts

This should be approximate and budget-aware. It is more important to give an
agent useful candidates than a perfect histogram. Counts require a bounded
per-path value-frequency counter, which is new machinery.

### Samples

Samples should remain bounded previews, not full record dumps by default.

Each sample should include:

- source
- record
- line, when known
- path
- value preview
- truncation flag

### Suggested Next Commands

The first version can generate simple deterministic suggestions:

```sh
jscan paths <input> --samples 2 --json
jscan shape <input> --json
```

Suggestions should be clearly labeled as commands, not authoritative queries.
Do not suggest `find` until that command exists. Command text should use the
resolved binary name once naming is settled.

### Suggested Next Tools

`profile` should help route the next step rather than pretending this tool owns
every task.

The first version can emit deterministic next-tool hints:

- `rg` for raw text smoke tests when structural safety is not required.
- `jq` or `jaq` for aggregation and value predicates.
- `jg` / `jsongrep` for field-presence or path-style discovery.
- this tool's `paths` / `shape` commands for deeper local reconnaissance.

Hints should include:

- tool name
- reason
- caveat
- example command when safe

Example:

```json
{
  "tool": "rg",
  "reason": "fast raw smoke test for a rare string",
  "caveat": "counts text occurrences, not matching JSON objects",
  "command": "rg 'dev.azure.com' <input>"
}
```

## CLI Shape

Initial command:

```sh
jscan profile [INPUT ...] --budget 20kb --json
```

Options:

- `--budget <size>`: output budget, accepting suffixes like `kb`, `mb`
- `--samples <n>`: max samples per high-signal path
- `--input-format auto|json|jsonl`
- `--all-files`
- `--strict`
- `--max-errors <n>`

Later options:

- `--embedded-json`
- `--record-root <path>`
- `--no-suggestions`
- `--focus security|api|logs|generic`

## Non-Goals

`profile` should not:

- generate Splunk/KQL/Grafana queries itself
- validate JSON Schema
- replace `shape`
- dump full records by default
- infer a perfect schema
- perform expensive full histograms unless requested
- parse embedded JSON by default

## MVP Implementation Strategy

Build `profile` from existing machinery first:

1. Reuse the input discovery and parsing behavior from `paths`.
2. Reuse path inventory facts.
3. Reuse shape inference facts.
4. Retain a per-source top-level summary during scanning:
   - effective input format
   - root kind
   - immediate object fields
   - dominant array fields
   - record counts
5. Add container-shape detection from that per-source summary.
6. Add record-root guessing.
7. Add advisory budgeted report assembly using top-N limits and section drops.
8. Add deterministic next-tool and next-command suggestions for existing
   commands only.

Avoid adding a query language while implementing `profile`.

v1 limitation: directory-wide profile should report per-source container
summaries, but detailed per-source path/shape breakdown beyond that can be
deferred.

## Tests

Add fixtures for:

- JSONL logs
- Splunk `{preview,result}` rows
- Splunk `{result}` rows
- top-level array snapshots
- paged API wrapper with `list`
- mixed directory with malformed and non-JSON files
- low-cardinality enum-like fields
- mixed-type fields
- budget truncation

Required checks:

- report remains valid JSON under budget
- high-priority metadata survives budget trimming
- malformed sources produce `partial` reports
- record-root guesses match expected wrappers
- common values are deterministic
- suggestions are deterministic

## Benchmark Hooks

The benchmark agent should evaluate whether `profile` reduces:

- number of exploratory commands needed
- total bytes emitted to the agent
- total wall time before a useful next query
- wrong-tool attempts, such as using raw text search where object-level
  structural semantics are required

Baseline comparison should include:

- repeated `jq`/`jaq` exploratory commands
- raw `rg` smoke tests
- `jscan paths` + `jscan shape`
- `jsongrep` where applicable
- `jsont schema` / `jsont tree` / `jsont find` where available
