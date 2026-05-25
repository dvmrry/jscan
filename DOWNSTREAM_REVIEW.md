# Downstream Profile Review Request

Please review the current prototype on representative private evidence data.

Repository/branch:

```text
https://github.com/dvmrry/jscan/tree/feature/research-plan
```

Build:

```sh
cargo build --release
```

Run:

```sh
target/release/jscan profile <input> --budget 20kb --json
```

## Review Goal

Evaluate whether `profile` reduces blind agent probing on private
network/security workflows.

This is not primarily a Rust/code review. The important question is:

> Does `jscan profile --budget 20kb --json` provide enough bounded structural
> context for an agent to write the next useful `jq`, `jaq`, `rg`, `jg`,
> Splunk, KQL, Grafana, or API query with fewer exploratory passes?

## Questions To Answer

1. Did it correctly detect JSON vs. NDJSON?
2. Did it identify Splunk `{preview,result}` or `{result}` wrappers?
3. Did it identify paged wrappers and top-level arrays?
4. Were `record_roots` useful and correct?
5. Were `path_facts` and `shape_facts` enough to write the next query?
6. Did `next_tools` give useful guidance or noise?
7. Was the output under or near the requested 20 KB budget?
8. What fields or facts were missing?
9. What was misleading?
10. Compared with previous manual `jq` / `jaq` / `rg` / `jg` probing, did
    `profile` replace any exploratory passes?

## Please Return

Return three concrete examples. Redact sensitive values.

For each example, include:

- input type
- approximate input size
- profile runtime
- profile output size
- one useful fact it surfaced
- one missing or misleading fact
- the next command/query you would run
- whether this replaced one or more exploratory passes

Then return:

- top 5 missing profile facts
- top 5 misleading/noisy profile facts
- whether the next implementation step should be:
  - improve `profile`
  - add focused grep/find
  - improve benchmarks
  - pause/rethink

## Safety

Do not share sensitive data.

Redact:

- customer names
- user names
- IPs
- domains
- tokens/secrets
- proprietary schema details
- raw event values that should not leave the private overlay

It is fine to share anonymized field/path shapes and timing/output-size
measurements.
