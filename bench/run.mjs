#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const benchDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(benchDir, "..");

const options = parseArgs(process.argv.slice(2));
const dataDir = resolve(repoRoot, options.dataDir);
const outDir = resolve(repoRoot, options.outDir);
const resultStem = new Date().toISOString().replaceAll(":", "").replace(/\.\d+Z$/, "Z");
const csvPath = join(outDir, `${resultStem}.csv`);
const mdPath = join(outDir, `${resultStem}.md`);
const latestCsvPath = join(outDir, "latest.csv");
const latestMdPath = join(outDir, "latest.md");
const jscanBin = resolve(repoRoot, options.jscanBin);

mkdirSync(dataDir, { recursive: true });
mkdirSync(outDir, { recursive: true });

if (options.regen) {
  rmSync(dataDir, { recursive: true, force: true });
  mkdirSync(dataDir, { recursive: true });
}

generateFixtures(dataDir);

if (options.build) {
  runRequired("cargo", ["build", "--release"], repoRoot);
}

const tools = {
  jscan: existsSync(jscanBin) ? jscanBin : null,
  jq: which("jq"),
  jaq: which("jaq"),
  rg: which("rg"),
  jg: which("jg"),
  jt: which("jt"),
  quicktype: which("quicktype"),
  gron: which("gron"),
  fastgron: which("fastgron"),
  duckdb: which("duckdb"),
  gensonCli: which("genson-cli"),
  jsonToSchema: which("json-to-schema"),
  schemax: which("schemax"),
  drivel: which("drivel"),
};

const fixtures = {
  splunk10k: join(dataDir, "splunk-10k.jsonl"),
  splunkArray10k: join(dataDir, "splunk-10k-array.json"),
  zia10k: join(dataDir, "zia-array-10k.json"),
  orders10k: join(dataDir, "orders-10k.jsonl"),
  paged5k: join(dataDir, "paged-wrapper-5k.json"),
  manySmall: join(dataDir, "many-small"),
  noisy: join(dataDir, "noisy"),
};

const tasks = benchmarkTasks(fixtures, tools);
const rows = [];

console.error(`bench data: ${relative(dataDir)}`);
console.error(`runs: ${options.runs}, warmups: ${options.warmups}`);
console.error(
  `tools: ${Object.entries(tools)
    .map(([name, value]) => `${name}=${value ? relative(value) : "missing"}`)
    .join(", ")}`,
);

for (const task of tasks) {
  const row = runBenchmark(task, options);
  rows.push(row);
  const status = row.status === "ok" ? `${row.median_ms.toFixed(2)} ms` : row.status;
  console.error(`${row.task} / ${row.tool}: ${status}`);
}

annotateCategories(rows);
writeCsv(csvPath, rows);
writeMarkdown(mdPath, rows, fixtures, tools, options);
writeCsv(latestCsvPath, rows);
writeMarkdown(latestMdPath, rows, fixtures, tools, options);

console.log(`wrote ${relative(csvPath)}`);
console.log(`wrote ${relative(mdPath)}`);

function parseArgs(args) {
  const parsed = {
    runs: numberFromEnv("JSCAN_BENCH_RUNS", 10),
    warmups: numberFromEnv("JSCAN_BENCH_WARMUPS", 2),
    dataDir: process.env.JSCAN_BENCH_DATA || "target/bench-data",
    outDir: process.env.JSCAN_BENCH_OUT || "target/bench-results",
    jscanBin: process.env.JSCAN_BIN || "target/release/jscan",
    build: process.env.JSCAN_BENCH_NO_BUILD !== "1",
    regen: false,
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--runs") {
      parsed.runs = parsePositiveInt(args[++index], "--runs");
    } else if (arg === "--warmups") {
      parsed.warmups = parsePositiveInt(args[++index], "--warmups");
    } else if (arg === "--data-dir") {
      parsed.dataDir = requiredValue(args[++index], "--data-dir");
    } else if (arg === "--out-dir") {
      parsed.outDir = requiredValue(args[++index], "--out-dir");
    } else if (arg === "--jscan-bin") {
      parsed.jscanBin = requiredValue(args[++index], "--jscan-bin");
    } else if (arg === "--no-build") {
      parsed.build = false;
    } else if (arg === "--regen") {
      parsed.regen = true;
    } else if (arg === "--help" || arg === "-h") {
      printHelpAndExit();
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }

  return parsed;
}

function numberFromEnv(name, fallback) {
  const value = process.env[name];
  return value ? parsePositiveInt(value, name) : fallback;
}

function parsePositiveInt(value, label) {
  const parsed = Number.parseInt(requiredValue(value, label), 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    fail(`${label} must be a non-negative integer`);
  }
  return parsed;
}

function requiredValue(value, label) {
  if (!value) {
    fail(`${label} requires a value`);
  }
  return value;
}

function printHelpAndExit() {
  console.log(`Usage: node bench/run.mjs [options]

Options:
  --runs N          measured runs per command (default: 10)
  --warmups N       warmup runs per command (default: 2)
  --data-dir PATH   fixture directory (default: target/bench-data)
  --out-dir PATH    result directory (default: target/bench-results)
  --jscan-bin PATH  jscan binary path (default: target/release/jscan)
  --no-build        skip cargo build --release
  --regen           regenerate fixture directory from scratch
`);
  process.exit(0);
}

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(2);
}

function generateFixtures(root) {
  writeSplunkJsonl(join(root, "splunk-10k.jsonl"), 10_000);
  writeSplunkArray(join(root, "splunk-10k-array.json"), 10_000);
  writeZiaArray(join(root, "zia-array-10k.json"), 10_000);
  writeOrdersJsonl(join(root, "orders-10k.jsonl"), 10_000);
  writePagedWrapper(join(root, "paged-wrapper-5k.json"), 5_000);
  writeManySmall(join(root, "many-small"), 200, 25);
  writeNoisyDir(join(root, "noisy"));
}

function writeSplunkJsonl(path, count) {
  const lines = [];
  for (let index = 0; index < count; index += 1) {
    lines.push(JSON.stringify(splunkWrapperRecord(index)));
  }
  writeFileSync(path, `${lines.join("\n")}\n`);
}

function writeSplunkArray(path, count) {
  const rows = [];
  for (let index = 0; index < count; index += 1) {
    rows.push(splunkWrapperRecord(index));
  }
  writeFileSync(path, `${JSON.stringify(rows)}\n`);
}

function splunkWrapperRecord(index) {
  const statuses = ["open", "close", "timeout", "reset"];
  const actions = ["ALLOW", "BLOCK", "INSPECT"];
  const result = {
    Host: `edge-${index % 97}`,
    ConnectionStatus: statuses[index % statuses.length],
    action: actions[index % actions.length],
    BytesIn: index * 17,
    BytesOut: index * 31,
    sourceIp: `192.0.2.${index % 250}`,
    destinationIp: `198.51.100.${(index * 7) % 250}`,
    connector: `connector-${index % 13}`,
    domainNames:
      index % 11 === 0
        ? ["dev.azure.com", `svc-${index % 53}.example.test`]
        : [`svc-${index % 53}.example.test`],
  };
  if (index % 70 === 0) {
    result.cribl_pipe = "barx";
  }
  if (index % 125 === 0) {
    result.ErrorMessage = "upstream timeout";
  }
  return { preview: false, result };
}

function writeZiaArray(path, count) {
  const rows = [];
  for (let index = 0; index < count; index += 1) {
    const row = {
      action: index % 5 === 0 ? "BLOCK" : "ALLOW",
      user: `user-${index % 400}@example.test`,
      url: `https://site-${index % 700}.example.test/path/${index}`,
      urlCategories: index % 3 === 0 ? ["Business", "Cloud Apps"] : ["Information Technology"],
      requestMethods: index % 2 === 0 ? ["GET"] : ["POST"],
      device: `device-${index % 120}`,
      sourceIp: `203.0.113.${index % 250}`,
    };
    if (index % 125 === 0) {
      row.endUserNotificationUrl = `https://notify.example.test/${index}`;
    }
    rows.push(row);
  }
  writeFileSync(path, `${JSON.stringify(rows)}\n`);
}

function writeOrdersJsonl(path, count) {
  const lines = [];
  for (let index = 0; index < count; index += 1) {
    lines.push(JSON.stringify({
      orderId: `order-${index}`,
      status: index % 7 === 0 ? "review" : "ok",
      items: [
        { sku: index % 5 === 0 ? "ABC" : `SKU-${index % 97}`, qty: (index % 4) + 1 },
        { sku: `ALT-${index % 53}`, qty: 1 },
      ],
      customer: {
        id: `cust-${index % 400}`,
        email: index % 9 === 0 ? `cust-${index % 400}@example.test` : null,
      },
    }));
  }
  writeFileSync(path, `${lines.join("\n")}\n`);
}

function writePagedWrapper(path, count) {
  const list = [];
  for (let index = 0; index < count; index += 1) {
    const item = {
      id: `app-${index}`,
      name: `Private App ${index}`,
      enabled: index % 7 !== 0,
      host: `app-${index % 300}.internal.example.test`,
    };
    if (index % 4 === 0) {
      item.domainNames = ["dev.azure.com", `app-${index % 300}.example.test`];
    }
    if (index % 31 === 0) {
      item.apiProtectionEnabled = true;
    }
    list.push(item);
  }
  writeFileSync(
    path,
    `${JSON.stringify({
      totalPages: 1,
      totalCount: count,
      list,
    })}\n`,
  );
}

function writeManySmall(dir, files, recordsPerFile) {
  mkdirSync(dir, { recursive: true });
  for (let fileIndex = 0; fileIndex < files; fileIndex += 1) {
    const list = [];
    for (let recordIndex = 0; recordIndex < recordsPerFile; recordIndex += 1) {
      list.push({
        id: `${fileIndex}-${recordIndex}`,
        action: recordIndex % 6 === 0 ? "BLOCK" : "ALLOW",
        host: `small-${fileIndex % 20}.example.test`,
        status: recordIndex % 10 === 0 ? "error" : "ok",
      });
    }
    writeFileSync(
      join(dir, `case-${String(fileIndex).padStart(4, "0")}.json`),
      `${JSON.stringify({ totalPages: 1, totalCount: recordsPerFile, list })}\n`,
    );
  }
}

function writeNoisyDir(dir) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "good.json"), `${JSON.stringify({ ok: true, list: [{ id: 1 }] })}\n`);
  writeFileSync(join(dir, "events.jsonl"), '{"event":"ok"}\n{"event":"bad","error":"timeout"}\n');
  writeFileSync(join(dir, "bad.json"), "{ this is not json\n");
  writeFileSync(join(dir, "notes.txt"), "not part of the default JSON scan\n");
}

function benchmarkTasks(f, t) {
  const splunkCombinedPredicate =
    'reduce inputs as $row (0; if ($row.result.Host? != null and $row.result.ConnectionStatus? == "timeout" and any(($row.result.domainNames? // [])[]; . == "dev.azure.com")) then . + 1 else . end)';
  const splunkRepeatedProbeCounts = [
    "sh",
    "-c",
    'host=$("$1" -n \'reduce inputs as $row (0; if $row.result.Host? != null then . + 1 else . end)\' "$2"); timeout=$("$1" -n \'reduce inputs as $row (0; if $row.result.ConnectionStatus? == "timeout" then . + 1 else . end)\' "$2"); azure=$("$1" -n \'reduce inputs as $row (0; if any(($row.result.domainNames? // [])[]; . == "dev.azure.com") then . + 1 else . end)\' "$2"); printf "%s,%s,%s\\n" "$host" "$timeout" "$azure"',
    "sh",
    t.jq,
    f.splunk10k,
  ];

  return [
    jscanTask("profile_splunk_10k", [t.jscan, "profile", f.splunk10k, "--budget", "20kb", "--json"], "bounded scout profile over Splunk-style JSONL"),
    jscanTask("paths_splunk_10k", [t.jscan, "paths", f.splunk10k, "--json"], "path/type inventory over JSONL"),
    jscanTask("paths_plain_splunk_10k", [t.jscan, "paths", f.splunk10k, "--plain"], "plain path list over JSONL; includes root and array-item paths", {
      answerFrom: stdoutLineCountAnswer,
    }),
    toolTask("paths_plain_splunk_10k", "jt", [t.jt, f.splunk10k, "fields"], "jsont compact field list over JSONL", {
      answerFrom: stdoutLineCountAnswer,
    }),
    jscanTask("paths_tsv_splunk_10k", [t.jscan, "paths", f.splunk10k], "TSV path/type/count inventory over JSONL", {
      answerFrom: stdoutLineCountAnswer,
    }),
    jscanTask("shape_splunk_10k", [t.jscan, "shape", f.splunk10k, "--json"], "object field optionality over JSONL"),
    jscanTask("profile_zia_array_10k", [t.jscan, "profile", f.zia10k, "--budget", "20kb", "--json"], "bounded scout profile over top-level array"),
    jscanTask("profile_paged_5k", [t.jscan, "profile", f.paged5k, "--budget", "20kb", "--json"], "bounded scout profile over paged wrapper"),
    jscanTask("profile_many_small", [t.jscan, "profile", f.manySmall, "--budget", "20kb", "--json"], "directory scan across many small JSON files"),
    jscanTask("paths_noisy_dir", [t.jscan, "paths", f.noisy, "--json"], "partial scan with malformed JSON"),

    toolTask("count_splunk_records", "jq", [t.jq, "-n", "reduce inputs as $row (0; . + 1)", f.splunk10k], "structural count of JSONL records", {
      contract: "record_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("count_splunk_records", "jaq", [t.jaq, "-n", "reduce inputs as $row (0; . + 1)", f.splunk10k], "structural count of JSONL records", {
      contract: "record_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("count_splunk_records", "rg", [t.rg, "-c", "^\\{", f.splunk10k], "raw text line count smoke test", {
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("count_splunk_records", "jt", [t.jt, f.splunk10k, "count"], "jsont JSONL record count", {
      contract: "record_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("count_splunk_records", "duckdb", [t.duckdb, "-csv", "-noheader", "-c", `select count(*) from read_json_auto('${sqlString(f.splunk10k)}')`], "DuckDB known-shape JSONL count", {
      contract: "record_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),

    toolTask("field_presence_connection_status", "jq", [t.jq, "-n", "reduce inputs as $row (0; if $row.result.ConnectionStatus? != null then . + 1 else . end)", f.splunk10k], "structural field-presence count", {
      contract: "field_presence_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("field_presence_connection_status", "jaq", [t.jaq, "-n", "reduce inputs as $row (0; if $row.result.ConnectionStatus? != null then . + 1 else . end)", f.splunk10k], "structural field-presence count", {
      contract: "field_presence_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("field_presence_connection_status", "rg", [t.rg, "-c", "\"ConnectionStatus\"", f.splunk10k], "raw text field occurrence count", {
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("field_presence_connection_status", "jg", [t.jg, "-f", "jsonl", "-F", "ConnectionStatus", "--count", "--no-display", "--porcelain", f.splunk10k], "JSON-aware field-presence scan", {
      contract: "field_presence_count",
      expectedAnswer: "10000",
      answerFrom: stdoutAnswer,
    }),
    jscanTask("field_presence_connection_status", [t.jscan, "paths", f.splunk10k, "--json"], "jscan path inventory includes $.result.ConnectionStatus count", {
      expectedAnswer: "10000",
      answerFrom: pathCountAnswer("$.result.ConnectionStatus"),
    }),
    toolTask("field_presence_connection_status", "jt", [t.jt, f.splunk10k, "fields"], "jsont field listing includes .result.ConnectionStatus", {
      expectedAnswer: "present",
      answerFrom: stdoutIncludesAnswer(".result.ConnectionStatus"),
    }),

    jscanTask("grep_combined_splunk", [t.jscan, "grep", f.splunk10k, "--has", "$.result.Host", "--eq", "$.result.ConnectionStatus", "timeout", "--contains", "$.result.domainNames", "dev.azure.com", "--count"], "one-pass multi-predicate structural count", {
      contract: "record_match_count",
      expectedAnswer: "227",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_combined_splunk", "jq", [t.jq, "-n", splunkCombinedPredicate, f.splunk10k], "single jq query with equivalent combined predicates", {
      contract: "record_match_count",
      expectedAnswer: "227",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_combined_splunk", "jaq", [t.jaq, "-n", splunkCombinedPredicate, f.splunk10k], "single jaq query with equivalent combined predicates", {
      contract: "record_match_count",
      expectedAnswer: "227",
      answerFrom: stdoutAnswer,
    }),
    jscanTask("multi_probe_counts_splunk", [t.jscan, "grep", f.splunk10k, "--has", "$.result.Host", "--eq", "$.result.ConnectionStatus", "timeout", "--contains", "$.result.domainNames", "dev.azure.com", "--json", "--limit", "0"], "one-pass independent counts for three structural probes", {
      expectedAnswer: "10000,2500,910",
      answerFrom: grepPredicateCountsAnswer,
    }),
    toolTask("multi_probe_counts_splunk", "jqx3", splunkRepeatedProbeCounts, "three separate jq probe counts over the same file", {
      expectedAnswer: "10000,2500,910",
      answerFrom: stdoutAnswer,
    }),

    jscanTask("grep_array_object_sku", [t.jscan, "grep", f.orders10k, "--some-eq", "$.items", "$.sku", "ABC", "--count"], "one-pass array-of-object item predicate", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_array_object_sku", "jq", [t.jq, "-n", 'reduce inputs as $row (0; if any(($row.items? // [])[]; .sku? == "ABC") then . + 1 else . end)', f.orders10k], "jq array-of-object item predicate", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_array_object_sku", "jaq", [t.jaq, "-n", 'reduce inputs as $row (0; if any(($row.items? // [])[]; .sku? == "ABC") then . + 1 else . end)', f.orders10k], "jaq array-of-object item predicate", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),

    jscanTask("grep_array_object_same_item", [t.jscan, "grep", f.orders10k, "--some", "$.items", "sku=ABC,qty=1", "--count"], "one-pass same-array-item multi-field predicate", {
      contract: "record_match_count",
      expectedAnswer: "500",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_array_object_same_item", "jq", [t.jq, "-n", 'reduce inputs as $row (0; if any(($row.items? // [])[]; .sku? == "ABC" and .qty? == 1) then . + 1 else . end)', f.orders10k], "jq same-array-item multi-field predicate", {
      contract: "record_match_count",
      expectedAnswer: "500",
      answerFrom: stdoutAnswer,
    }),
    toolTask("grep_array_object_same_item", "jaq", [t.jaq, "-n", 'reduce inputs as $row (0; if any(($row.items? // [])[]; .sku? == "ABC" and .qty? == 1) then . + 1 else . end)', f.orders10k], "jaq same-array-item multi-field predicate", {
      contract: "record_match_count",
      expectedAnswer: "500",
      answerFrom: stdoutAnswer,
    }),

    toolTask("filter_zia_block", "jq", [t.jq, "[.[] | select(.action == \"BLOCK\")] | length", f.zia10k], "structural value predicate over top-level array", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("filter_zia_block", "jaq", [t.jaq, "[.[] | select(.action == \"BLOCK\")] | length", f.zia10k], "structural value predicate over top-level array", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("filter_zia_block", "rg", [t.rg, "--count-matches", "\"action\":\"BLOCK\"", f.zia10k], "raw text value occurrence count", {
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),
    toolTask("filter_zia_block", "duckdb", [t.duckdb, "-csv", "-noheader", "-c", `select count(*) from read_json_auto('${sqlString(f.zia10k)}') where action = 'BLOCK'`], "DuckDB known-shape value predicate over array JSON", {
      contract: "record_match_count",
      expectedAnswer: "2000",
      answerFrom: stdoutAnswer,
    }),

    toolTask("path_inventory_splunk", "jq", [t.jq, "-n", "reduce inputs as $row ({}; reduce ($row | paths) as $p (. ; .[$p | map(tostring) | join(\".\")] = true)) | length", f.splunk10k], "jq path inventory baseline", {
      contract: "distinct_path_count",
      expectedAnswer: "15",
      answerFrom: stdoutAnswer,
    }),
    toolTask("path_inventory_splunk", "jaq", [t.jaq, "-n", "reduce inputs as $row ({}; reduce ($row | paths) as $p (. ; .[$p | map(tostring) | join(\".\")] = true)) | length", f.splunk10k], "jaq path inventory baseline", {
      contract: "distinct_path_count",
      expectedAnswer: "15",
      answerFrom: stdoutAnswer,
    }),
    jscanTask("path_inventory_splunk", [t.jscan, "paths", f.splunk10k, "--json"], "jscan native path/type inventory", {
      answerFrom: pathsLengthAnswer,
    }),
    toolTask("path_inventory_splunk", "jt", [t.jt, f.splunk10k, "fields"], "jsont field inventory over JSONL", {
      answerFrom: stdoutLineCountAnswer,
    }),

    toolTask("schema_splunk_jsonl", "jt", [t.jt, f.splunk10k, "schema"], "jsont schema/frequency profile over JSONL"),
    toolTask("schema_splunk_jsonl", "quicktype", [t.quicktype, "--lang", "schema", f.splunk10k], "quicktype direct JSONL input; expected to fail on line-delimited JSON", {
      expectedStatus: "exit:1",
    }),
    toolTask("schema_splunk_array", "quicktype", [t.quicktype, "--lang", "schema", f.splunkArray10k], "quicktype schema after JSONL is normalized to an array"),
    toolTask("schema_paged_wrapper", "quicktype", [t.quicktype, "--lang", "schema", f.paged5k], "quicktype schema over paged JSON wrapper"),
    toolTask("schema_splunk_jsonl", "genson-cli", [t.gensonCli, "--ndjson", f.splunk10k], "genson-cli schema inference over NDJSON"),
    toolTask("schema_paged_wrapper", "genson-cli", [t.gensonCli, f.paged5k], "genson-cli schema inference over paged JSON wrapper"),
    toolTask("schema_splunk_array", "json-to-schema", [t.jsonToSchema, "-i", f.splunkArray10k], "json-to-schema inference after JSONL is normalized to an array"),
    toolTask("schema_paged_wrapper", "json-to-schema", [t.jsonToSchema, "-i", f.paged5k], "json-to-schema inference over paged JSON wrapper"),
    toolTask("schema_splunk_array", "schemax", [t.schemax, "infer", f.splunkArray10k], "schemax inference after JSONL is normalized to an array"),
    toolTask("schema_paged_wrapper", "schemax", [t.schemax, "infer", f.paged5k], "schemax inference over paged JSON wrapper"),
    toolTask("schema_paged_wrapper", "drivel", [t.drivel, "describe", "--json-schema"], "drivel schema inference over paged JSON wrapper", {
      stdinFile: f.paged5k,
    }),

    toolTask("flatten_zia_array", "gron", [t.gron, f.zia10k], "gron flattening baseline over top-level JSON array"),
    toolTask("flatten_zia_array", "fastgron", [t.fastgron, f.zia10k], "fastgron flattening baseline over top-level JSON array"),
    toolTask("flatten_paged_wrapper", "gron", [t.gron, f.paged5k], "gron flattening baseline over paged wrapper"),
    toolTask("flatten_paged_wrapper", "fastgron", [t.fastgron, f.paged5k], "fastgron flattening baseline over paged wrapper"),
  ];
}

function jscanTask(task, command, note, extra = {}) {
  return {
    task,
    tool: "jscan",
    command,
    semantic: "json_structural",
    contract: extra.contract ?? `${task}:jscan`,
    note,
    ...extra,
  };
}

function toolTask(task, tool, command, note, extra = {}) {
  return {
    task,
    tool,
    command,
    semantic: tool === "rg" ? "raw_text" : "json_structural",
    contract: extra.contract ?? `${task}:${tool}`,
    note,
    ...extra,
  };
}

function runBenchmark(task, opts) {
  if (!task.command[0]) {
    return missingRow(task);
  }

  const stdinBuffer = task.stdinFile ? readFileSync(task.stdinFile) : null;
  for (let index = 0; index < opts.warmups; index += 1) {
    runOnce(task.command, stdinBuffer);
  }

  const runs = [];
  let last = null;
  for (let index = 0; index < opts.runs; index += 1) {
    last = runOnce(task.command, stdinBuffer);
    runs.push(last);
  }

  const statuses = new Set(runs.map((run) => run.status));
  let status = statuses.size === 1 && statuses.has(0) ? "ok" : `exit:${[...statuses].join("|")}`;
  const answers = runs.map((run) => answerFor(task, run));
  if (status === "ok" && answers.some((answer) => answer.error)) {
    status = "answer_error";
  } else if (
    status === "ok"
    && task.expectedAnswer !== undefined
    && answers.some((answer) => answer.value !== task.expectedAnswer)
  ) {
    status = "wrong_answer";
  }
  if (task.expectedStatus && status === task.expectedStatus) {
    status = "expected_failure";
  }
  const times = runs.map((run) => run.elapsedMs).sort((a, b) => a - b);
  const lastAnswer = answers.at(-1);
  const stdoutBytes = last?.stdout.length ?? 0;
  const stderrBytes = last?.stderr.length ?? 0;
  const stdoutLines = countLines(last?.stdout ?? Buffer.alloc(0));
  const stdoutSha256 = createHash("sha256").update(last?.stdout ?? Buffer.alloc(0)).digest("hex").slice(0, 16);

  return {
    task: task.task,
    tool: task.tool,
    semantic: task.semantic,
    status,
    runs: opts.runs,
    median_ms: median(times),
    min_ms: times[0] ?? 0,
    max_ms: times[times.length - 1] ?? 0,
    stdout_bytes: stdoutBytes,
    stdout_lines: stdoutLines,
    stderr_bytes: stderrBytes,
    stdout_sha256: stdoutSha256,
    answer: lastAnswer?.error ? `error:${lastAnswer.error}` : (lastAnswer?.value ?? ""),
    expected_answer: task.expectedAnswer ?? "",
    contract: task.contract,
    category: "",
    command: commandDisplay(task.command),
    note: task.note,
  };
}

function answerFor(task, run) {
  if (!task.answerFrom || run.status !== 0) {
    return { value: "" };
  }

  try {
    return { value: task.answerFrom(run.stdout) };
  } catch (error) {
    return { value: "", error: error.message };
  }
}

function stdoutAnswer(stdout) {
  return stdout.toString("utf8").trim();
}

function stdoutLineCountAnswer(stdout) {
  return String(countLines(stdout));
}

function stdoutIncludesAnswer(needle) {
  return (stdout) => (stdout.toString("utf8").includes(needle) ? "present" : "missing");
}

function pathCountAnswer(displayPath) {
  return (stdout) => {
    const report = JSON.parse(stdout.toString("utf8"));
    const entry = report.paths.find((path) => path.display_path === displayPath);
    return String(entry?.count ?? "");
  };
}

function pathsLengthAnswer(stdout) {
  const report = JSON.parse(stdout.toString("utf8"));
  return String(report.paths.length);
}

function grepPredicateCountsAnswer(stdout) {
  const report = JSON.parse(stdout.toString("utf8"));
  return report.predicates.map((predicate) => String(predicate.matched_records)).join(",");
}

function runOnce(command, stdinBuffer = null) {
  const start = process.hrtime.bigint();
  const result = spawnSync(command[0], command.slice(1), {
    cwd: repoRoot,
    env: { ...process.env, NO_COLOR: "1" },
    input: stdinBuffer,
    maxBuffer: 512 * 1024 * 1024,
  });
  const end = process.hrtime.bigint();
  return {
    status: typeof result.status === "number" ? result.status : 128,
    elapsedMs: Number(end - start) / 1_000_000,
    stdout: result.stdout ?? Buffer.alloc(0),
    stderr: result.stderr ?? Buffer.alloc(0),
  };
}

function missingRow(task) {
  return {
    task: task.task,
    tool: task.tool,
    semantic: task.semantic,
    status: "missing",
    runs: 0,
    median_ms: 0,
    min_ms: 0,
    max_ms: 0,
    stdout_bytes: 0,
    stdout_lines: 0,
    stderr_bytes: 0,
    stdout_sha256: "",
    answer: "",
    expected_answer: task.expectedAnswer ?? "",
    contract: task.contract,
    category: "",
    command: commandDisplay(task.command),
    note: task.note,
  };
}

function annotateCategories(rows) {
  const groups = new Map();
  for (const row of rows) {
    if (!groups.has(row.task)) {
      groups.set(row.task, []);
    }
    groups.get(row.task).push(row);
  }

  for (const row of rows) {
    if (row.status === "missing") {
      row.category = "missing_optional_tool";
    } else if (row.status === "expected_failure") {
      row.category = "expected_failure";
    }
  }

  for (const group of groups.values()) {
    const comparable = group.filter((row) => row.category === "");
    if (comparable.length <= 1) {
      for (const row of comparable) {
        row.category = "solo_coverage";
      }
      continue;
    }

    const contractGroups = new Map();
    for (const row of comparable) {
      if (!contractGroups.has(row.contract)) {
        contractGroups.set(row.contract, []);
      }
      contractGroups.get(row.contract).push(row);
    }

    for (const contractGroup of contractGroups.values()) {
      const category = isFairRace(contractGroup)
        ? "fair_competitor_race"
        : "contract_or_answer_mismatch";
      for (const row of contractGroup) {
        row.category = category;
      }
    }
  }
}

function isFairRace(rows) {
  if (rows.length < 2 || rows.some((row) => row.status !== "ok" || !row.answer)) {
    return false;
  }

  return new Set(rows.map((row) => row.answer)).size === 1;
}

function median(values) {
  if (values.length === 0) {
    return 0;
  }
  const mid = Math.floor(values.length / 2);
  if (values.length % 2 === 1) {
    return values[mid];
  }
  return (values[mid - 1] + values[mid]) / 2;
}

function countLines(buffer) {
  if (buffer.length === 0) {
    return 0;
  }
  let lines = 0;
  for (const byte of buffer) {
    if (byte === 10) {
      lines += 1;
    }
  }
  return lines;
}

function writeCsv(path, rows) {
  const columns = [
    "task",
    "tool",
    "category",
    "contract",
    "semantic",
    "status",
    "runs",
    "median_ms",
    "min_ms",
    "max_ms",
    "stdout_bytes",
    "stdout_lines",
    "stderr_bytes",
    "stdout_sha256",
    "answer",
    "expected_answer",
    "command",
    "note",
  ];
  const lines = [columns.join(",")];
  for (const row of rows) {
    lines.push(columns.map((column) => csvCell(row[column])).join(","));
  }
  writeFileSync(path, `${lines.join("\n")}\n`);
}

function writeMarkdown(path, rows, fixtures, tools, opts) {
  const fixtureRows = Object.entries(fixtures)
    .map(([name, value]) => `| ${name} | ${relative(value)} | ${formatBytes(pathSize(value))} |`)
    .join("\n");
  const toolRows = Object.entries(tools)
    .map(([name, value]) => `| ${name} | ${value ? relative(value) : "missing"} |`)
    .join("\n");
  const categoryRows = [
    ["fair_competitor_race", "Fair competitor races"],
    ["contract_or_answer_mismatch", "Contract or answer mismatch rows"],
    ["solo_coverage", "Solo coverage rows"],
    ["expected_failure", "Expected failure rows"],
    ["missing_optional_tool", "Missing optional tools"],
  ]
    .map(([category, label]) => `| ${label} | ${rows.filter((row) => row.category === category).length} |`)
    .join("\n");
  const resultSections = [
    ["fair_competitor_race", "Fair Competitor Races"],
    ["contract_or_answer_mismatch", "Contract Or Answer Mismatch Rows"],
    ["solo_coverage", "Solo Coverage Rows"],
    ["expected_failure", "Expected Failure Rows"],
    ["missing_optional_tool", "Missing Optional Tools"],
  ]
    .map(([category, title]) => resultSection(title, rows.filter((row) => row.category === category)))
    .join("\n\n");

  writeFileSync(
    path,
    `# Benchmark Results

Generated: ${new Date().toISOString()}

Runs: ${opts.runs}
Warmups: ${opts.warmups}

## Tools

| Tool | Path |
| --- | --- |
${toolRows}

## Fixtures

| Fixture | Path | Size |
| --- | --- | ---: |
${fixtureRows}

## Scorecard

| Category | Rows |
| --- | ---: |
${categoryRows}

## Results

${resultSections}

Full command strings, output hashes, answer validation fields, and categories are
in the CSV result next to this file.
`,
  );
}

function resultSection(title, rows) {
  if (rows.length === 0) {
    return `### ${title}

No rows.`;
  }

  const resultRows = rows
    .map(
      (row) =>
        `| ${row.task} | ${row.tool} | ${escapeMd(row.contract)} | ${row.semantic} | ${row.status} | ${row.runs} | ${row.median_ms.toFixed(2)} | ${row.stdout_bytes} | ${escapeMd(row.answer)} | ${escapeMd(row.expected_answer)} | ${escapeMd(row.note)} |`,
    )
    .join("\n");

  return `### ${title}

| Task | Tool | Contract | Semantic | Status | Runs | Median ms | Stdout bytes | Answer | Expected | Note |
| --- | --- | --- | --- | --- | ---: | ---: | ---: | --- | --- | --- |
${resultRows}
`;
}

function csvCell(value) {
  const text = String(value ?? "");
  if (/[",\n]/.test(text)) {
    return `"${text.replaceAll('"', '""')}"`;
  }
  return text;
}

function commandDisplay(command) {
  return command.map((part) => (part ? shellQuote(part) : "<missing>")).join(" ");
}

function shellQuote(value) {
  if (/^[A-Za-z0-9_./:=@%+-]+$/.test(value)) {
    return value;
  }
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function escapeMd(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

function which(name) {
  const result = spawnSync("which", [name], { encoding: "utf8" });
  if (result.status !== 0) {
    return null;
  }
  return result.stdout.trim() || null;
}

function sqlString(value) {
  return String(value).replaceAll("'", "''");
}

function runRequired(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function relative(path) {
  if (!path) {
    return "";
  }
  if (path.startsWith(repoRoot)) {
    return path.slice(repoRoot.length + 1);
  }
  return path;
}

function pathSize(path) {
  const stats = statSync(path);
  if (stats.isFile()) {
    return stats.size;
  }
  let total = 0;
  for (const name of readdirSync(path)) {
    total += pathSize(join(path, name));
  }
  return total;
}

function formatBytes(bytes) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
