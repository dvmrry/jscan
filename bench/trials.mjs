#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
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

generateTrialFixtures(dataDir);

if (options.build) {
  runRequired("cargo", ["build", "--release"], repoRoot);
}

const tools = {
  jscan: existsSync(jscanBin) ? jscanBin : null,
  jq: which("jq"),
  rg: which("rg"),
};

const fixtures = {
  splunkJsonNamedJson: join(dataDir, "splunk-export.json"),
  ziaArray: join(dataDir, "zia-array.json"),
  pagedWrapper: join(dataDir, "paged-wrapper.json"),
  bracketWrapper: join(dataDir, "bracket-wrapper.json"),
};

const rows = [];
for (const trial of workflowTrials(fixtures, tools)) {
  for (const workflow of trial.workflows) {
    const row = runWorkflow(trial, workflow, options);
    rows.push(row);
    const status = row.status === "ok" ? `${row.median_ms.toFixed(2)} ms` : row.status;
    console.error(`${row.trial} / ${row.workflow}: ${status}`);
  }
}

writeCsv(csvPath, rows);
writeCsv(latestCsvPath, rows);
writeMarkdown(mdPath, rows, fixtures, tools, options);
writeMarkdown(latestMdPath, rows, fixtures, tools, options);

console.log(`wrote ${relative(csvPath)}`);
console.log(`wrote ${relative(mdPath)}`);

function parseArgs(args) {
  const parsed = {
    runs: numberFromEnv("JSCAN_TRIAL_RUNS", 5),
    warmups: numberFromEnv("JSCAN_TRIAL_WARMUPS", 1),
    dataDir: process.env.JSCAN_TRIAL_DATA || "target/trial-data",
    outDir: process.env.JSCAN_TRIAL_OUT || "target/trial-results",
    jscanBin: process.env.JSCAN_BIN || "target/release/jscan",
    build: process.env.JSCAN_TRIAL_NO_BUILD !== "1",
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
  console.log(`Usage: node bench/trials.mjs [options]

Options:
  --runs N          measured runs per workflow (default: 5)
  --warmups N       warmup runs per workflow (default: 1)
  --data-dir PATH   fixture directory (default: target/trial-data)
  --out-dir PATH    result directory (default: target/trial-results)
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

function generateTrialFixtures(root) {
  writeSplunkJsonNamedJson(join(root, "splunk-export.json"), 10_000);
  writeZiaArray(join(root, "zia-array.json"), 10_000);
  writePagedWrapper(join(root, "paged-wrapper.json"), 5_000);
  writeBracketWrapper(join(root, "bracket-wrapper.json"), 1_000);
}

function writeSplunkJsonNamedJson(path, count) {
  const statuses = ["open", "close", "timeout", "reset"];
  const lines = [];
  for (let index = 0; index < count; index += 1) {
    lines.push(
      JSON.stringify({
        preview: false,
        result: {
          Host: `edge-${index % 80}`,
          ConnectionStatus: statuses[index % statuses.length],
          action: index % 3 === 0 ? "BLOCK" : "ALLOW",
          sourceIp: `192.0.2.${index % 250}`,
          destinationIp: `198.51.100.${(index * 7) % 250}`,
          domainNames: index % 8 === 0 ? ["dev.azure.com"] : [`svc-${index % 90}.example.test`],
        },
      }),
    );
  }
  writeFileSync(path, `${lines.join("\n")}\n`);
}

function writeZiaArray(path, count) {
  const rows = [];
  for (let index = 0; index < count; index += 1) {
    const row = {
      action: index % 5 === 0 ? "BLOCK" : "ALLOW",
      user: `user-${index % 400}@example.test`,
      url: `https://site-${index % 500}.example.test/path/${index}`,
      urlCategories: ["Business", index % 2 === 0 ? "Cloud Apps" : "Information Technology"],
      requestMethods: [index % 2 === 0 ? "GET" : "POST"],
    };
    if (index % 125 === 0) {
      row.endUserNotificationUrl = `https://notify.example.test/${index}`;
    }
    rows.push(row);
  }
  writeFileSync(path, `${JSON.stringify(rows)}\n`);
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
    list.push(item);
  }
  writeFileSync(path, `${JSON.stringify({ totalPages: 1, totalCount: count, list })}\n`);
}

function writeBracketWrapper(path, count) {
  const items = [];
  for (let index = 0; index < count; index += 1) {
    const item = {
      id: `item-${index}`,
      status: index % 3 === 0 ? "active" : "idle",
    };
    if (index % 10 === 0) {
      item.rare = true;
    }
    items.push(item);
  }
  writeFileSync(path, `${JSON.stringify({ "items-list": items })}\n`);
}

function workflowTrials(f, t) {
  return [
    {
      trial: "splunk_multi_probe_counts",
      input: f.splunkJsonNamedJson,
      expected: "10000,2500,1250",
      question: "Collect independent structural probe counts for Host presence, timeout status, and dev.azure.com domain membership.",
      workflows: [
        workflow(
          "jscan_grep_one_pass",
          "one_pass_structural_probe",
          [
            [
              t.jscan,
              "grep",
              f.splunkJsonNamedJson,
              "--has",
              "$.result.Host",
              "--eq",
              "$.result.ConnectionStatus",
              "timeout",
              "--contains",
              "$.result.domainNames",
              "dev.azure.com",
              "--json",
              "--limit",
              "0",
            ],
          ],
          { answerFrom: grepPredicateCountsAnswer },
        ),
        workflow(
          "blind_jq_probes",
          "blind_probe_then_query",
          [
            jqJsonl(t.jq, "reduce inputs as $row (0; if $row.result.Host? != null then . + 1 else . end)", f.splunkJsonNamedJson),
            jqJsonl(t.jq, "reduce inputs as $row (0; if $row.result.ConnectionStatus? == \"timeout\" then . + 1 else . end)", f.splunkJsonNamedJson),
            jqJsonl(t.jq, "reduce inputs as $row (0; if any(($row.result.domainNames? // [])[]; . == \"dev.azure.com\") then . + 1 else . end)", f.splunkJsonNamedJson),
          ],
          { answerFrom: stepStdoutCsvAnswer },
        ),
      ],
    },
    {
      trial: "splunk_timeout_count",
      input: f.splunkJsonNamedJson,
      expected: "2500",
      question: "Count Splunk rows whose result.ConnectionStatus is timeout in a JSONL file named .json.",
      workflows: [
        workflow("oracle_jq", "oracle_known_query", [jqJsonl(t.jq, "reduce inputs as $row (0; if $row.result.ConnectionStatus == \"timeout\" then . + 1 else . end)", f.splunkJsonNamedJson)]),
        workflow("blind_jq_probes", "blind_probe_then_query", [
          jqJsonl(t.jq, "input | keys", f.splunkJsonNamedJson),
          jqJsonl(t.jq, "input.result | keys", f.splunkJsonNamedJson),
          jqJsonl(t.jq, "reduce inputs as $row (0; if $row.result.ConnectionStatus == \"timeout\" then . + 1 else . end)", f.splunkJsonNamedJson),
        ]),
        workflow("profile_then_jq", "profile_assisted", [
          [t.jscan, "profile", f.splunkJsonNamedJson, "--budget", "20kb", "--json"],
          jqJsonl(t.jq, "reduce inputs as $row (0; if $row.result.ConnectionStatus == \"timeout\" then . + 1 else . end)", f.splunkJsonNamedJson),
        ]),
        workflow("raw_rg", "raw_text", [rgCount(t.rg, "\"ConnectionStatus\":\"timeout\"", f.splunkJsonNamedJson)]),
      ],
    },
    {
      trial: "zia_notification_count",
      input: f.ziaArray,
      expected: "80",
      question: "Count top-level array records that include endUserNotificationUrl.",
      workflows: [
        workflow("oracle_jq", "oracle_known_query", [jqJson(t.jq, "[.[] | select(.endUserNotificationUrl? != null)] | length", f.ziaArray)]),
        workflow("blind_jq_probes", "blind_probe_then_query", [
          jqJson(t.jq, ".[0] | keys", f.ziaArray),
          jqJson(t.jq, "[.[] | select(.endUserNotificationUrl? != null)] | length", f.ziaArray),
        ]),
        workflow("profile_then_jq", "profile_assisted", [
          [t.jscan, "profile", f.ziaArray, "--budget", "20kb", "--json"],
          jqJson(t.jq, "[.[] | select(.endUserNotificationUrl? != null)] | length", f.ziaArray),
        ]),
        workflow("raw_rg", "raw_text", [rgCount(t.rg, "\"endUserNotificationUrl\"", f.ziaArray)]),
      ],
    },
    {
      trial: "paged_dev_domain_count",
      input: f.pagedWrapper,
      expected: "1250",
      question: "Count paged-wrapper list records whose domainNames include dev.azure.com.",
      workflows: [
        workflow("oracle_jq", "oracle_known_query", [jqJson(t.jq, "[.list[] | select((.domainNames // []) | index(\"dev.azure.com\"))] | length", f.pagedWrapper)]),
        workflow("blind_jq_probes", "blind_probe_then_query", [
          jqJson(t.jq, "keys", f.pagedWrapper),
          jqJson(t.jq, ".list[0] | keys", f.pagedWrapper),
          jqJson(t.jq, "[.list[] | select((.domainNames // []) | index(\"dev.azure.com\"))] | length", f.pagedWrapper),
        ]),
        workflow("profile_then_jq", "profile_assisted", [
          [t.jscan, "profile", f.pagedWrapper, "--budget", "20kb", "--json"],
          jqJson(t.jq, "[.list[] | select((.domainNames // []) | index(\"dev.azure.com\"))] | length", f.pagedWrapper),
        ]),
        workflow("raw_rg", "raw_text", [rgCount(t.rg, "\"dev.azure.com\"", f.pagedWrapper)]),
      ],
    },
    {
      trial: "bracket_key_rare_count",
      input: f.bracketWrapper,
      expected: "100",
      question: "Count records under a dominant top-level array field with a bracket-only key.",
      workflows: [
        workflow("oracle_jq", "oracle_known_query", [jqJson(t.jq, "[.[\"items-list\"][] | select(.rare == true)] | length", f.bracketWrapper)]),
        workflow("blind_jq_probes", "blind_probe_then_query", [
          jqJson(t.jq, "keys", f.bracketWrapper),
          jqJson(t.jq, ".[\"items-list\"][0] | keys", f.bracketWrapper),
          jqJson(t.jq, "[.[\"items-list\"][] | select(.rare == true)] | length", f.bracketWrapper),
        ]),
        workflow("profile_then_jq", "profile_assisted", [
          [t.jscan, "profile", f.bracketWrapper, "--budget", "20kb", "--json"],
          jqJson(t.jq, "[.[\"items-list\"][] | select(.rare == true)] | length", f.bracketWrapper),
        ]),
      ],
    },
  ];
}

function workflow(name, kind, steps, extra = {}) {
  return { name, kind, steps, ...extra };
}

function jqJsonl(jq, filter, input) {
  return [jq, "-n", filter, input];
}

function jqJson(jq, filter, input) {
  return [jq, filter, input];
}

function rgCount(rg, pattern, input) {
  return [rg, "--count-matches", pattern, input];
}

function runWorkflow(trial, workflow, opts) {
  if (workflow.steps.some((step) => !step[0])) {
    return missingRow(trial, workflow);
  }

  for (let index = 0; index < opts.warmups; index += 1) {
    runSteps(workflow.steps);
  }

  const runs = [];
  for (let index = 0; index < opts.runs; index += 1) {
    runs.push(runSteps(workflow.steps));
  }

  const statuses = new Set(runs.flatMap((run) => run.steps.map((step) => step.status)));
  let status = statuses.size === 1 && statuses.has(0) ? "ok" : `exit:${[...statuses].join("|")}`;
  const answers = runs.map((run) => answerForWorkflow(workflow, run));
  if (status === "ok" && answers.some((answer) => answer !== trial.expected)) {
    status = "wrong_answer";
  }

  const times = runs.map((run) => run.elapsedMs).sort((a, b) => a - b);
  const lastRun = runs.at(-1);
  const lastStdout = lastRun?.lastStdout ?? Buffer.alloc(0);
  return {
    trial: trial.trial,
    workflow: workflow.name,
    kind: workflow.kind,
    status,
    runs: opts.runs,
    calls: workflow.steps.length,
    median_ms: median(times),
    min_ms: times[0] ?? 0,
    max_ms: times.at(-1) ?? 0,
    stdout_bytes: lastRun?.stdoutBytes ?? 0,
    stderr_bytes: lastRun?.stderrBytes ?? 0,
    answer: answers.at(-1) ?? "",
    expected_answer: trial.expected,
    final_stdout_sha256: createHash("sha256").update(lastStdout).digest("hex").slice(0, 16),
    commands: workflow.steps.map(commandDisplay).join(" ; "),
    question: trial.question,
  };
}

function runSteps(steps) {
  const start = process.hrtime.bigint();
  const results = [];
  let stdoutBytes = 0;
  let stderrBytes = 0;
  for (const step of steps) {
    const result = spawnSync(step[0], step.slice(1), {
      cwd: repoRoot,
      env: { ...process.env, NO_COLOR: "1" },
      maxBuffer: 512 * 1024 * 1024,
    });
    const stdout = result.stdout ?? Buffer.alloc(0);
    const stderr = result.stderr ?? Buffer.alloc(0);
    stdoutBytes += stdout.length;
    stderrBytes += stderr.length;
    results.push({
      status: typeof result.status === "number" ? result.status : 128,
      stdout,
      stderr,
    });
    if (result.status !== 0) {
      break;
    }
  }
  const end = process.hrtime.bigint();
  return {
    elapsedMs: Number(end - start) / 1_000_000,
    stdoutBytes,
    stderrBytes,
    lastStdout: results.at(-1)?.stdout ?? Buffer.alloc(0),
    steps: results,
  };
}

function missingRow(trial, workflow) {
  return {
    trial: trial.trial,
    workflow: workflow.name,
    kind: workflow.kind,
    status: "missing",
    runs: 0,
    calls: workflow.steps.length,
    median_ms: 0,
    min_ms: 0,
    max_ms: 0,
    stdout_bytes: 0,
    stderr_bytes: 0,
    answer: "",
    expected_answer: trial.expected,
    final_stdout_sha256: "",
    commands: workflow.steps.map(commandDisplay).join(" ; "),
    question: trial.question,
  };
}

function stdoutAnswer(stdout) {
  return stdout.toString("utf8").trim();
}

function answerForWorkflow(workflow, run) {
  return workflow.answerFrom ? workflow.answerFrom(run) : stdoutAnswer(run.lastStdout);
}

function grepPredicateCountsAnswer(run) {
  const report = JSON.parse(run.lastStdout.toString("utf8"));
  return report.predicates.map((predicate) => String(predicate.matched_records)).join(",");
}

function stepStdoutCsvAnswer(run) {
  return run.steps.map((step) => stdoutAnswer(step.stdout)).join(",");
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

function writeCsv(path, rows) {
  const columns = [
    "trial",
    "workflow",
    "kind",
    "status",
    "runs",
    "calls",
    "median_ms",
    "min_ms",
    "max_ms",
    "stdout_bytes",
    "stderr_bytes",
    "answer",
    "expected_answer",
    "final_stdout_sha256",
    "commands",
    "question",
  ];
  const lines = [columns.join(",")];
  for (const row of rows) {
    lines.push(columns.map((column) => csvCell(row[column])).join(","));
  }
  writeFileSync(path, `${lines.join("\n")}\n`);
}

function writeMarkdown(path, rows, fixtures, tools, opts) {
  const toolRows = Object.entries(tools)
    .map(([name, value]) => `| ${name} | ${value ? relative(value) : "missing"} |`)
    .join("\n");
  const fixtureRows = Object.entries(fixtures)
    .map(([name, value]) => `| ${name} | ${relative(value)} | ${formatBytes(statSync(value).size)} |`)
    .join("\n");
  const resultRows = rows
    .map(
      (row) =>
        `| ${row.trial} | ${row.workflow} | ${row.kind} | ${row.status} | ${row.calls} | ${row.median_ms.toFixed(2)} | ${row.stdout_bytes} | ${escapeMd(row.answer)} | ${escapeMd(row.expected_answer)} |`,
    )
    .join("\n");

  writeFileSync(
    path,
    `# Workflow Trial Results

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

## Results

| Trial | Workflow | Kind | Status | Calls | Median ms | Stdout bytes | Answer | Expected |
| --- | --- | --- | --- | ---: | ---: | ---: | --- | --- |
${resultRows}

The CSV file includes full command sequences and question text.
`,
  );
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

function runRequired(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
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

function formatBytes(bytes) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
