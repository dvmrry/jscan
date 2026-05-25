use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Map, Value, json};
use tempfile::tempdir;

#[test]
fn paths_outputs_pretty_table() {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");

    cmd.arg("paths")
        .arg("tests/fixtures/basic.json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("$.users[].email")
                .and(predicate::str::contains("string:1"))
                .and(predicate::str::contains("null:1")),
        );
}

#[test]
fn paths_outputs_stable_json_with_errors() {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");

    cmd.arg("paths")
        .arg("tests/fixtures/events.jsonl")
        .arg("--json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"schema\": \"jscan.paths.v1\"")
                .and(predicate::str::contains(
                    "\"display_path\": \"$.error.message\"",
                ))
                .and(predicate::str::contains("\"line\": 3")),
        );
}

#[test]
fn shape_reports_optional_fields() {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");

    cmd.arg("shape")
        .arg("tests/fixtures/events.jsonl")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("status\trequired\tstring:3")
                .and(predicate::str::contains("error\t33.3%\tobject:1")),
        );
}

#[test]
fn directory_scan_reports_bad_json_file_without_aborting() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("good.json"), r#"{"ok": true}"#).expect("good json");
    fs::write(dir.path().join("notes.txt"), "not json").expect("text file");
    fs::write(dir.path().join("bad.json"), [0xff, 0xfe]).expect("bad json");

    let output = command_json(
        &["paths", dir.path().to_str().expect("utf-8 path"), "--json"],
        None,
    );
    let sources = output["sources"].as_array().expect("sources");
    let source_names = sources
        .iter()
        .map(|source| source["source"].as_str().expect("source"))
        .collect::<Vec<_>>();

    assert!(
        source_names
            .iter()
            .any(|source| source.ends_with("good.json"))
    );
    assert!(
        source_names
            .iter()
            .any(|source| source.ends_with("bad.json"))
    );
    assert!(
        !source_names
            .iter()
            .any(|source| source.ends_with("notes.txt"))
    );
    assert_eq!(output["partial"], true);
    assert_eq!(output["error_count"], 1);
    assert!(output["paths"].to_string().contains("$.ok"));
}

#[test]
fn malformed_multiline_json_does_not_fallback_to_line_errors() {
    let output = command_json(&["paths", "--json"], Some("{\n  \"a\": 1,\n  BROKEN\n}\n"));

    assert_eq!(output["error_count"], 1);
    assert_eq!(output["errors"].as_array().expect("errors").len(), 1);
    assert_eq!(output["errors"][0]["line"], 3);
    assert!(output["paths"].as_array().expect("paths").is_empty());
}

#[test]
fn reports_error_truncation() {
    let output = command_json(
        &[
            "paths",
            "--json",
            "--input-format",
            "jsonl",
            "--max-errors",
            "2",
        ],
        Some("bad\nbad\nbad\n"),
    );

    assert_eq!(output["partial"], true);
    assert_eq!(output["error_count"], 3);
    assert_eq!(output["errors_truncated"], true);
    assert_eq!(output["errors"].as_array().expect("errors").len(), 2);
}

#[test]
fn samples_are_bounded_and_include_source_line() {
    let output = command_json(
        &[
            "paths",
            "--json",
            "--input-format",
            "jsonl",
            "--samples",
            "1",
            "--sample-max-chars",
            "3",
        ],
        Some("\nnot json\n{\"msg\":\"abcdef\"}\n"),
    );
    let msg = output["paths"]
        .as_array()
        .expect("paths")
        .iter()
        .find(|entry| entry["display_path"] == "$.msg")
        .expect("msg path");
    let sample = &msg["samples"][0];

    assert_eq!(sample["record"], 1);
    assert_eq!(sample["line"], 3);
    assert_eq!(sample["value_preview"], "abc...");
    assert_eq!(sample["truncated"], true);
}

#[test]
fn shape_json_reports_array_items() {
    let output = command_json(&["shape", "tests/fixtures/basic.json", "--json"], None);
    let users = output["arrays"]
        .as_array()
        .expect("arrays")
        .iter()
        .find(|entry| entry["display_path"] == "$.users")
        .expect("users array");

    assert_eq!(users["count"], 1);
    assert_eq!(users["item_count"], 2);
    assert_eq!(users["item_types"]["object"], 2);
}

#[test]
fn strict_exits_nonzero_on_partial_scan() {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");

    cmd.arg("paths")
        .arg("--input-format")
        .arg("jsonl")
        .arg("--strict")
        .write_stdin("bad\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("scan completed with 1 error"));
}

#[test]
fn profile_detects_splunk_result_wrapper() {
    let output = command_json(
        &[
            "profile",
            "tests/fixtures/splunk.jsonl",
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );

    assert_eq!(output["schema"], "jscan.profile.v1");
    assert_json_array_contains(&output["containers"], "kind", "jsonl_records");
    assert_json_array_contains(
        &output["containers"],
        "kind",
        "splunk_preview_result_wrapper",
    );
    assert_json_array_contains(&output["record_roots"], "display_path", "$.result");
    assert_json_array_contains(
        &output["path_facts"],
        "display_path",
        "$.result.ConnectionStatus",
    );
    assert_json_array_contains(&output["next_tools"], "tool", "rg");
    assert_json_array_contains(&output["next_tools"], "command", "jaq -c '.result' <input>");
}

#[test]
fn profile_detects_paged_list_wrapper() {
    let output = command_json(
        &[
            "profile",
            "tests/fixtures/paged.json",
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );

    assert_json_array_contains(&output["containers"], "kind", "paged_list_wrapper");
    assert_json_array_contains(&output["record_roots"], "display_path", "$.list[]");
    assert_json_array_contains(
        &output["path_facts"],
        "display_path",
        "$.list[].domainNames[]",
    );
    assert!(
        output["next_tools"]
            .as_array()
            .expect("next_tools")
            .iter()
            .any(|tool| tool["command"]
                == "jaq -c '.list[] | select(.domainNames? != null)' <input>"),
        "expected a next-tool command to use .list[] and a selective path"
    );
}

#[test]
fn profile_detects_top_level_array() {
    let output = command_json(
        &[
            "profile",
            "tests/fixtures/array.json",
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );

    assert_json_array_contains(&output["containers"], "kind", "root_array");
    assert_json_array_contains(&output["record_roots"], "display_path", "$[]");
    assert_json_array_contains(&output["path_facts"], "display_path", "$[].action");
    assert_json_array_contains(&output["next_tools"], "command", "jaq -c '.[]' <input>");
}

#[test]
fn profile_top_level_array_next_tool_uses_dot_array_wildcard() {
    let output = command_json(
        &["profile", "--json", "--budget", "20kb"],
        Some(
            r#"[
                {"id": 1, "action": "ALLOW"},
                {"id": 2, "action": "BLOCK", "endUserNotificationUrl": "https://notify.example.test"}
            ]"#,
        ),
    );

    assert_json_array_contains(&output["record_roots"], "display_path", "$[]");
    assert_json_array_contains(
        &output["next_tools"],
        "command",
        "jaq -c '.[] | select(.endUserNotificationUrl? != null)' <input>",
    );
    assert!(
        !output["next_tools"]
            .as_array()
            .expect("next_tools")
            .iter()
            .any(|tool| tool["command"]
                .as_str()
                .expect("command")
                .starts_with("jaq -c '[]")),
        "root arrays must render as .[], not []"
    );
}

#[test]
fn profile_dominant_array_field_with_bracket_key_uses_dot_bracket_access() {
    let output = command_json(
        &["profile", "--json", "--budget", "20kb"],
        Some(
            r#"{
                "items-list": [
                    {"id": 1, "status": "present"},
                    {"id": 2, "status": "present", "rare": true}
                ]
            }"#,
        ),
    );

    assert_json_array_contains(
        &output["record_roots"],
        "display_path",
        "$[\"items-list\"][]",
    );
    assert_json_array_contains(
        &output["next_tools"],
        "command",
        "jaq -c '.[\"items-list\"][] | select(.rare? != null)' <input>",
    );
    assert_json_array_contains(
        &output["next_tools"],
        "command",
        "jg '$[\"items-list\"][].rare' <input>",
    );
    assert!(
        !output["next_tools"]
            .as_array()
            .expect("next_tools")
            .iter()
            .any(|tool| tool["command"]
                .as_str()
                .expect("command")
                .starts_with("jaq -c '[\"items-list\"]")),
        "root bracket access must render as .[\"items-list\"], not an array literal"
    );
}

#[test]
fn profile_budget_reports_omissions_when_trimmed() {
    let output = command_json(
        &[
            "profile",
            "tests/fixtures/basic.json",
            "--json",
            "--budget",
            "1kb",
        ],
        None,
    );

    assert_eq!(output["budget"]["requested_bytes"], 1024);
    assert_eq!(output["budget"]["truncated"], true);
    assert!(
        !output["budget"]["omitted"]
            .as_array()
            .expect("omitted")
            .is_empty()
    );
}

#[test]
fn profile_reports_jsonl_after_auto_fallback_for_json_file() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("splunk-export.json");
    fs::write(
        &input,
        concat!(
            "{\"preview\":false,\"result\":{\"ConnectionStatus\":\"OPEN\"}}\n",
            "{\"preview\":false,\"result\":{\"ConnectionStatus\":\"CLOSED\"}}\n"
        ),
    )
    .expect("write fixture");

    let output = command_json(
        &[
            "profile",
            input.to_str().expect("utf-8 path"),
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );

    assert_eq!(output["sources"][0]["format"], "jsonl");
    assert_json_array_contains(&output["containers"], "kind", "jsonl_records");
    assert_json_array_contains(&output["record_roots"], "display_path", "$.result");
}

#[test]
fn profile_budget_matches_emitted_json_and_preserves_evidence() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("wide.jsonl");
    let mut lines = Vec::new();
    for record_index in 0..12 {
        let mut result = Map::new();
        result.insert(
            "ConnectionStatus".to_string(),
            json!(if record_index % 2 == 0 {
                "OPEN"
            } else {
                "CLOSED"
            }),
        );
        result.insert("Host".to_string(), json!(format!("host-{record_index}")));
        result.insert(
            "sourceIp".to_string(),
            json!(format!("192.0.2.{record_index}")),
        );
        result.insert(
            "description".to_string(),
            json!(format!("record description {record_index}")),
        );
        result.insert(
            "domainNames".to_string(),
            json!(["dev.azure.com", "example.test"]),
        );
        for field_index in 0..70 {
            result.insert(
                format!("field{field_index}"),
                json!(format!("value-{record_index}-{field_index}")),
            );
        }

        lines.push(json!({"preview": false, "result": Value::Object(result)}).to_string());
    }
    fs::write(&input, lines.join("\n")).expect("write fixture");

    let stdout = command_stdout(
        &[
            "profile",
            input.to_str().expect("utf-8 path"),
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );
    let output: Value = serde_json::from_slice(&stdout).expect("json output");

    assert!(
        stdout.len() <= 20 * 1024,
        "profile exceeded budget: {} bytes",
        stdout.len()
    );
    assert_eq!(
        output["budget"]["estimated_bytes"]
            .as_u64()
            .expect("estimated bytes"),
        stdout.len() as u64
    );
    assert!(
        !output["samples"].as_array().expect("samples").is_empty(),
        "budgeting should preserve representative samples at 20kb"
    );
    assert!(
        !output["common_values"]
            .as_array()
            .expect("common_values")
            .is_empty(),
        "budgeting should preserve representative common values at 20kb"
    );
}

#[test]
fn profile_budget_bounds_many_source_directories() {
    let dir = tempdir().expect("tempdir");
    for file_index in 0..120 {
        fs::write(
            dir.path().join(format!("case-{file_index:03}.json")),
            format!(
                r#"{{"totalPages":1,"totalCount":1,"list":[{{"id":"case-{file_index}","status":"ok"}}]}}"#
            ),
        )
        .expect("write fixture");
    }

    let stdout = command_stdout(
        &[
            "profile",
            dir.path().to_str().expect("utf-8 path"),
            "--json",
            "--budget",
            "20kb",
        ],
        None,
    );
    let output: Value = serde_json::from_slice(&stdout).expect("json output");

    assert!(
        stdout.len() <= 20 * 1024,
        "profile exceeded budget: {} bytes",
        stdout.len()
    );
    assert_eq!(
        output["budget"]["estimated_bytes"]
            .as_u64()
            .expect("estimated bytes"),
        stdout.len() as u64
    );
    assert_eq!(output["budget"]["truncated"], true);
    assert!(
        json_strings(&output["budget"]["omitted"]).contains(&"sources_tail".to_string()),
        "expected sources_tail omission for large directory profile"
    );
}

#[test]
fn profile_keyword_signals_are_token_aware() {
    let output = command_json(
        &["profile", "--json", "--budget", "20kb"],
        Some(
            r#"{"description":"letters that used to trigger ip","apiProtectionEnabled":true,"sourceIp":"192.0.2.1"}"#,
        ),
    );

    let description = find_path_fact(&output, "$.description");
    let api_protection = find_path_fact(&output, "$.apiProtectionEnabled");
    let source_ip = find_path_fact(&output, "$.sourceIp");

    assert!(!json_strings(&description["signals"]).contains(&"keyword:ip".to_string()));
    assert!(!json_strings(&api_protection["signals"]).contains(&"keyword:ip".to_string()));
    assert!(json_strings(&source_ip["signals"]).contains(&"keyword:source".to_string()));
    assert!(json_strings(&source_ip["signals"]).contains(&"keyword:ip".to_string()));
}

fn command_json(args: &[&str], stdin: Option<&str>) -> Value {
    let output = command_stdout(args, stdin);
    serde_json::from_slice(&output).expect("json output")
}

fn command_stdout(args: &[&str], stdin: Option<&str>) -> Vec<u8> {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");
    cmd.args(args);
    if let Some(stdin) = stdin {
        cmd.write_stdin(stdin);
    }

    cmd.assert().success().get_output().stdout.clone()
}

fn assert_json_array_contains(array: &Value, key: &str, expected: &str) {
    let values = array.as_array().expect("json array");
    assert!(
        values.iter().any(|value| value[key] == expected),
        "expected array to contain {key}={expected}, got {values:#?}"
    );
}

fn find_path_fact(output: &Value, path: &str) -> Value {
    output["path_facts"]
        .as_array()
        .expect("path_facts")
        .iter()
        .find(|entry| entry["display_path"] == path)
        .unwrap_or_else(|| panic!("missing path fact {path}"))
        .clone()
}

fn json_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}
