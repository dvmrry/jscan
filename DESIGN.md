# Design Notes

## Product Boundary

The tool is for reconnaissance, not transformation.

Good questions:

- What paths exist in this data?
- Which fields are optional?
- What types have been observed at this path?
- Which JSONL records contain this shape?
- Where is the evidence, and how can I inspect a bounded preview?

Non-goals:

- Replacing `jq` or `jaq`
- Implementing a general value transformation language
- Being a TUI-first JSON viewer
- Treating dotted display paths as canonical identifiers

Canonical location data should remain structured:

- segment arrays
- JSON Pointer-compatible templates
- source file
- record number
- source line when known

Display paths are for humans.

## Output Contract

JSON output is the agent-facing contract. Pretty output is for humans.

Reports should include:

- `schema`
- `partial`
- `error_count`
- `errors_truncated`
- `sources`
- command-specific data
- bounded `errors`

Parse/read failures should be data in the report whenever possible. A scan over
multiple files should continue after a bad source.

`--strict` converts partial scans into non-zero exits after writing the report.

## Find Contract

`find` must locate, not extract.

It should produce match evidence that can be piped to another tool for
transformation. It should avoid becoming a jq-like expression language.

The query AST should be stable and machine-writable before any compact DSL is
added.
