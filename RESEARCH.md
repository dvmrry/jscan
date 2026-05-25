# Research Plan

We paused implementation because there are two related but different product
visions in play.

## The Two Visions

### Vision A: The Scout

A small Unix-style helper for humans and agents that do not understand the JSON
yet.

Typical commands:

```sh
jsonq paths data.jsonl
jsonq shape data.jsonl
jsonq sample --path /user/email data.jsonl
jsonq find --evidence query.json data.jsonl
```

Primary value:

- orientation
- path/type discovery
- schema or shape inference
- bounded samples
- source/line/path evidence
- agent-friendly JSON output

Relationship to `jq`: complements it.

### Vision B: The Engine

A fast structural query tool that replaces broad or slow `jq` searches in many
common cases.

Typical commands:

```sh
jsonq 'object(status="failed", error.message:string)' logs.jsonl
jsonq get 'orders[].items[] where sku =~ "^ABC"' dump.json
jsonq find query.json --values logs.jsonl
jsonq find query.json --count logs.jsonl
```

Primary value:

- fast broad search
- streaming JSONL evaluation
- structural predicates
- extraction of matching values or subtrees
- fewer trips through `jq`

Relationship to `jq`: overlaps with part of its query role, but should not grow
into a full transformation language.

## Working Hypothesis

Build the Scout first, but design the internals so the Engine can grow from it.

Early external behavior should emphasize evidence:

```sh
jsonq find --query q.json logs.jsonl
```

Later output modes may serve the Engine vision:

```sh
jsonq find --query q.json --values logs.jsonl
jsonq find --query q.json --count logs.jsonl
jsonq find --query q.json --ndjson logs.jsonl
```

The key boundary:

> Query for structure, location, and matching values. Do not become a general
> JSON programming and transformation language.

## Research Question

What is the best wedge?

- agent-friendly JSON reconnaissance
- fast structural query engine
- a hybrid with discovery first and query delivery second

## Field Notes From Target Workflow

Primary target pain:

- Network/security investigation workflows over logs and API outputs.
- Agents often run several `jq` / `jaq` passes over the same document or log set.
- The user sees this as dead-air time: repeated "thinking..." steps and repeated
  exploratory shell commands before the agent has enough context.
- When agents have a full schema or strong field map, generated Splunk,
  Grafana/KQL, API, and local JSON queries are much better.

Common search shapes mentioned:

- key exists
- key/value match
- arrays containing multiple terms or matching objects
- broad search across raw logs
- examples of matching records
- compact context for downstream agent query generation

Inputs to support:

- JSONL logs
- giant JSON arrays
- API response objects
- Splunk-exported or security-tool logs
- mixed directories of saved responses and schemas

Important uncertainty:

- The user is not asking for a theoretical query language. The useful product
  may be the thing that avoids repeated blind jq probing by doing a fast
  one-pass profile and then targeted structural grep.
- Embedded JSON in raw log text may be useful, but only if it can be optional
  or cheap enough not to undermine speed.

### Private Overlay Agent Report

A data-adjacent agent reviewed private workflow data and recommended the product
be **Scout/Locator first**, not a `jq` / `jaq` replacement.

Observed local corpus, redacted and summarized:

- 402 JSON-named files under data, cases, schemas, and infrastructure folders
  after excluding workflow internals.
- 46 `.json` files were actually NDJSON or multiple JSON documents, totaling
  roughly 62,724 records.
- Container shapes were mixed:
  - ZIA snapshots: top-level arrays.
  - ZPA snapshots: paged objects like `{totalPages,totalCount,list}`.
  - Splunk exports: NDJSON rows shaped like `{preview,result}` even when named
    `.json`.
  - Some Splunk `_raw` fields contained parseable embedded JSON.

Concrete workflow signals:

- Some investigations had capped Splunk exports and repeated follow-up
  aggregates because raw export shape and coverage mattered.
- Some workflows loaded schema mappings only after a bad or placeholder query
  path had already been attempted.
- Distinguishing wildcard ZIA rows, projected ZPA rows, and raw ZPA connector
  rows mattered in real cases.
- Existing schema notes already encode hard-won field mappings, such as
  differences between ZPA host/destination fields, ZIA web fields, and firewall
  destination fields.

Recommended practical wedge from that agent:

```sh
jscan profile evidence.json --budget 20kb --json
jscan paths evidence.json --samples 2 --json
jscan find --has result.Host --has result.ConnectionStatus --limit 20 --json
```

Key value statement:

> The value is not "write a better filter than jq." It is: in one bounded pass,
> tell the agent "this is NDJSON, records are under result, these are the
> observed fields/types/optional fields, and here are source lines and small
> samples." Then the agent can write correct jq, jaq, Splunk, KQL, or API
> queries with fewer blind probes.

Important caveat:

- The private agent could not run the Rust prototype in its environment because
  `cargo` was unavailable, so this validation was based on local data inspection
  with existing tools rather than direct prototype execution.

### Initial Private Benchmark Report

A second data-adjacent pass ran a small local benchmark against representative
private artifacts. The benchmark runner used 10 runs per command and reported
median seconds. `jsongrep` was installed via Homebrew as `jg` version 0.9.0.

Representative results, redacted/summarized:

| Task | jq | jaq | rg | jg |
| --- | ---: | ---: | ---: | ---: |
| Count 10k Splunk NDJSON records | 0.248s | 0.120s | 0.046s | n/a |
| Structured NDJSON filter, count close + nonzero bytes | 0.267s | 0.145s | 0.050s | n/a |
| Field presence in NDJSON: `ConnectionStatus` | 0.260s | 0.138s | 0.040s | 0.126s |
| Heterogeneous path inventory on BARX export | 0.182s | 0.298s | n/a | n/a |
| ZIA array filter: `action == BLOCK` | 0.028s | 0.025s | 0.030s | n/a |
| ZPA wrapper domain match: `dev.azure.com` | 0.035s | 0.042s | 0.033s | n/a |
| ZPA `domainNames` field presence | 0.023s | 0.029s | 0.032s | 0.024s |

Important semantic note:

- In one ZPA domain match, raw `rg` returned 4 text occurrences while
  JSON-aware `jq`/`jaq` returned 1 matching application object. This is the core
  raw-speed vs. structural-safety tradeoff.

Benchmark takeaways:

- `rg` is the fastest useful smoke-test tool.
- `jaq` was materially faster than `jq` on larger NDJSON streaming tasks.
- `jq` was better than `jaq` for at least one path-inventory expression.
- `jg` should stay in the competitor set: for field-presence/path-style
  discovery it is fast, ergonomic, and JSONL-aware.
- `jg` does not replace `jq`/`jaq` for aggregation and value predicates in these
  flows.

Product implication:

> The tool should not compete with `rg` on raw speed or with `jq`/`jaq` on
> transformation. It should own bounded structural reconnaissance: detect JSON
> vs. NDJSON, expose paths/types/samples/source lines, and tell the agent which
> of `rg`, `jq`, `jaq`, or `jg` is the right next tool.

### Downstream Profile Review

The private-data reviewer ran `jscan profile --budget 20kb --json` against
representative evidence and confirmed the concept is useful, but found several
profile-contract problems to fix before adding `find`:

- `.json` files that auto-fallback to NDJSON were parsed correctly but reported
  as `format: auto`, which hid the `jsonl_records` container.
- `budget.estimated_bytes` used compact JSON while `--json` emitted pretty JSON,
  so actual output could exceed the requested budget.
- The budget reducer dropped all samples and common values before trimming lower
  value report tails, leaving agents with paths but no local value hints.
- Keyword detection used substring matches, so fields like `description` could
  be tagged as `keyword:ip`.
- `next_tools` was static and did not use detected record roots such as
  `$.result`, `$.list[]`, or `$[]`.

The same review returned three concrete private examples:

- Splunk `{preview,result}` NDJSON in a `.json` file: profile found 10k records
  and `record_roots: $.result`, replacing format/root discovery probes.
- ZPA paged wrapper: profile found `paged_list_wrapper`, `record_roots:
  $.list[]`, and `domainNames[]`, replacing a field-selection probe.
- ZIA top-level array: profile found `root_array` and useful arrays such as
  `urlCategories[]` and `requestMethods[]`, though value hints were missing
  before the budget fix.

Top missing facts from that review:

- effective parsed format after auto fallback
- actual emitted-byte size under the requested budget
- bounded low-cardinality values or redacted samples
- omitted-count metadata for capped facts
- record-root-specific next commands

Recommendation from the review:

> Improve `profile` first. The concept is real and useful for agentic JSON work,
> but fix format truthfulness, budget accounting, value preservation, and
> adaptive guidance before adding focused grep/find.

### Downstream Rerun After Profile Fixes

The private-data reviewer reran profile on the follow-up commit and confirmed
the main previous issues were fixed:

- `.json` Splunk NDJSON now reports `format: jsonl` and includes
  `jsonl_records`.
- Budget estimates match emitted pretty JSON byte size.
- 20 KB profiles preserve samples and common values.
- `description` and `apiProtectionEnabled` no longer get false `keyword:ip`
  signals.
- BARX mixed-type fields report `mixed_types` without the old noisy IP signal.

Private rerun examples stayed within budget:

- ADO profile: 18,572 bytes in 0.14s.
- BARX profile: 20,239 bytes in 0.02s.
- ZPA wrapper profile: 19,570 bytes in 0.01s.
- ZIA root-array profile: 20,335 bytes in 0.01s.

New findings from that rerun:

- Top-level array next-tool commands rendered a root wildcard as `[]` instead
  of `.[]`, so `jaq -c '[] | select(...)'` returned no matches where
  `jaq -c '.[] | select(...)'` was correct.
- Generated `select(path? != null)` filters were structurally valid for
  wrappers, but often non-selective because the chosen field existed on every
  record. In those cases the command validated the record root but did not
  narrow the next query.
- Dominant top-level array fields whose key requires bracket syntax had the
  same root-expression bug. A wrapper like `{"items-list":[...]}` produced
  `["items-list"][]`, which queries an array literal; the correct jq/jaq root is
  `.["items-list"][]`. The display root should also be `$["items-list"][]`,
  not `$.items-list[]`.

Adjustment:

- Root wildcard rendering should produce `.[]` for top-level arrays.
- Root bracket-key rendering should produce `.[...]`, not a bracket expression
  with no leading dot.
- Dominant-array record-root display paths should use bracket syntax for keys
  that are not jq identifiers.
- Next-tool hints should only emit a scalar presence `select(...)` when the
  candidate path is narrower than the detected record root. If no narrower path
  is available, the hint should project the record root and say no selective
  predicate was found.

Candidate workflow to test:

```sh
jsonq profile logs.jsonl --budget 20kb --json
jsonq grep logs.jsonl --has error.message --has user.id --examples 5
jsonq grep logs.jsonl --array-contains events '{ "action": "blocked" }'
```

Measurement should include not only raw speed, but also:

- number of commands an agent needs before it can produce a good query
- total bytes/tokens emitted to the agent
- whether source/line/path evidence is sufficient to avoid another pass
- whether the output improves generated Splunk/Grafana/KQL/API queries
- whether the profile can recommend the right next local tool:
  - `rg` for raw smoke tests
  - `jq`/`jaq` for structural aggregation/value predicates
  - `jg` for path/field-presence discovery

### Local Benchmark Harness Smoke

A repo-local CLI harness now lives at `bench/run.mjs`. It generates
deterministic synthetic fixtures, builds `target/release/jscan`, skips missing
competitor tools, and writes CSV/Markdown results under `target/bench-results`.
Rows now include semantic `Answer` and `Expected` fields for tasks where a
stable answer is known; commands that exit zero with the wrong answer are marked
`wrong_answer`.

Initial local smoke run, 3 measured runs with 1 warmup:

| Task | Tool | Median |
| --- | --- | ---: |
| Splunk 10k bounded profile | `jscan` | 55.68 ms |
| Splunk 10k path inventory | `jscan` | 55.05 ms |
| Splunk 10k record count | `jq` | 38.86 ms |
| Splunk 10k record count | `rg` | 3.77 ms |
| Splunk 10k field presence | `jq` | 41.10 ms |
| Splunk 10k field presence | `rg` | 3.74 ms |
| ZIA 10k `action == BLOCK` | `jq` | 33.69 ms |
| ZIA 10k raw `BLOCK` count | `rg` | 4.15 ms |
| Splunk 10k path inventory | `jq` | 509.72 ms |
| Splunk 10k path inventory | `jscan` | 54.99 ms |

Local machine caveats:

- `jaq` and `jg` were not installed locally, so those rows were recorded as
  missing.
- `rg` is expected to win raw text smoke tests, but those tasks are marked
  `raw_text` because they are not structurally safe JSON record predicates.
- The first harness run exposed a directory-profile budget leak. `profile` now
  truncates high-cardinality source/container/root tails when needed; the same
  many-small fixture now emits 15,747 bytes under a 20 KB budget.

Downstream harness review found two false-positive benchmark rows in the first
version:

- `jg '$.result.ConnectionStatus'` against JSONL exited zero but emitted zero
  bytes. The harness now uses `jg -f jsonl -F ConnectionStatus --count
  --no-display --porcelain` for that task and validates the expected answer
  `10000`. `--porcelain` matters because otherwise `jg` may emit human text
  like `Found matches: 10000` instead of a bare count.
- `rg -c '"action":"BLOCK"'` counted matching lines, not matches, because the
  ZIA fixture is a single-line JSON array. The harness now uses
  `rg --count-matches` for occurrence-count comparison and validates the
  expected answer `2000`.

### Local Workflow Trial Smoke

A second harness, `bench/trials.mjs`, measures complete workflows to a correct
answer:

- `oracle_jq`: the final known-good query only.
- `blind_jq_probes`: exploratory jq shape probes plus the final query.
- `profile_then_jq`: `jscan profile --budget 20kb --json` plus the final query.
- `raw_rg`: raw text count where meaningful.

Initial local smoke run, 3 measured runs with 1 warmup:

| Trial | Workflow | Calls | Median | Stdout bytes |
| --- | --- | ---: | ---: | ---: |
| Splunk timeout count | `oracle_jq` | 1 | 29.64 ms | 5 |
| Splunk timeout count | `blind_jq_probes` | 3 | 35.63 ms | 130 |
| Splunk timeout count | `profile_then_jq` | 2 | 75.14 ms | 12,289 |
| Splunk timeout count | `raw_rg` | 1 | 3.57 ms | 5 |
| ZIA notification count | `oracle_jq` | 1 | 27.66 ms | 3 |
| ZIA notification count | `blind_jq_probes` | 2 | 53.36 ms | 104 |
| ZIA notification count | `profile_then_jq` | 2 | 59.77 ms | 10,573 |
| Paged dev-domain count | `blind_jq_probes` | 3 | 36.85 ms | 111 |
| Paged dev-domain count | `profile_then_jq` | 2 | 31.09 ms | 11,957 |
| Bracket-key rare count | `blind_jq_probes` | 3 | 11.23 ms | 56 |
| Bracket-key rare count | `profile_then_jq` | 2 | 7.35 ms | 7,064 |

Read this cautiously:

- `profile_then_jq` does not beat known-query `jq` or raw `rg`; it should not.
- It only wins in synthetic trials where the profile replaces enough
  exploratory probes, such as the paged wrapper and bracket-key wrapper.
- The private/raw-data trial should decide whether those wins exist in real
  agent workflows, and whether the extra profile bytes are worth the reduced
  probing.

## Competitor Set

Primary:

- `jq`
- `jaq`
- `jsongrep` / `jg`
- `jsont` / `jt`

Secondary:

- `gron`
- `fastgron`
- `ripgrep` over raw JSON
- `duckdb` for table-shaped JSONL
- JSONPath / JMESPath / JSONata CLIs if a mature command-line baseline exists

## Competitor Questions

For each competitor, answer:

- What is its mental model?
- What is easy?
- What is awkward?
- How does it handle JSONL?
- Can it scan many files?
- Can it scan noisy directories?
- Does it emit source, line, path, and bounded evidence?
- Does it infer paths or shape?
- Can it output matching values?
- Can it count matches?
- Where does it get slow?
- What command would an agent have to write for broad structural search?

## Use Case Corpus

Write concrete tasks before designing more syntax. Each task should be phrased
as a user need, not as a feature.

Seed tasks:

1. I have 4 GB of JSONL logs. Find records where an error object has a message
   and a user ID.
2. I have a HAR file. Show what request and response fields exist.
3. I have a GitHub API dump. Find objects where `permissions.admin` is true.
4. I have unknown JSON and need to keep output under 20 KB while learning its
   structure.
5. I have logs where `status` sometimes changes type. Show the paths and sample
   values.
6. I have a directory with mixed JSON, text, and broken files. Report usable
   JSON without aborting.
7. I need every path that can contain a token-like key.
8. I need examples of records that contain `user.email`, without dumping the
   entire record.
9. I need to count JSONL records where an array contains an object with
   `sku == "ABC"`.
10. I need matching values as NDJSON so another command can consume them.
11. I need to discover optional fields in a stream of event objects.
12. I need to find nested objects shaped like `{id, name, email?}`.
13. I need to compare how many records have `error.message` across many files.
14. I need source line and JSON path for every match.
15. I need to scan a huge top-level JSON array without loading everything.
16. I need a command an agent can run before writing a custom parser.
17. I need to know whether a field is enum-like and see common values.
18. I need to search for a string value structurally, not as raw text.
19. I need path discovery across thousands of small JSON files.
20. I need parse errors summarized without hiding that more errors were
   truncated.

## Task Classification

For every task, classify it as:

- Scout
- Engine
- Both
- Not ours

Also record:

- best `jq` command
- best `jaq` command
- best `jsongrep` command
- best `jsont` command
- best raw `rg`/`gron`/other command where applicable
- whether the command is obvious enough for an agent to generate safely
- expected output shape
- what our ideal command would be

## Benchmark Sketch

Benchmarks should follow the tasks, not the other way around.

Discovery benchmarks:

- path inventory
- shape discovery
- bounded samples
- many small files
- noisy/malformed directories

Search benchmarks:

- key anywhere
- path pattern
- string value anywhere
- object containing field set
- nested array element predicate
- count-only query
- value extraction query

Stress benchmarks:

- huge JSONL
- huge top-level array
- high match count
- rare match
- deeply nested input
- mixed valid and invalid files

Metrics:

- wall time
- peak memory
- output size
- exit behavior
- parse-error behavior
- command complexity

## Name Check

Do not finalize the name until the wedge is chosen.

If the product is mostly Scout:

- `json-recon`
- `jrecon`
- `json-locate`
- `json-scout`

If the product is more Engine:

- `jsonq`
- `jsift`
- `jwhere`

The name should match expectations. `jsonq` is plausible for the Engine vision,
but it invites comparison with JSON query/transformation languages.

## First Place To Start

Start with the use case corpus.

Reason: competitor research without concrete tasks can become a museum tour.
Tasks reveal whether the real wedge is Scout, Engine, or hybrid.

Immediate next step:

1. Treat the private overlay report as initial evidence for Scout/Locator first.
2. Define what `profile --budget 20kb --json` must include.
3. Pick 5 to 8 seed tasks from the corpus above that match the observed private
   workflow shapes.
4. For each task, write the best command in `jq`, `jaq`, `jsongrep`, and
   `jsont`.
5. Mark each task Scout, Engine, Both, or Not ours.
6. Only then decide whether `jsonq` is the right name.
