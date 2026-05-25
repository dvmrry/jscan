use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
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

fn command_json(args: &[&str], stdin: Option<&str>) -> Value {
    let mut cmd = Command::cargo_bin("jscan").expect("binary");
    cmd.args(args);
    if let Some(stdin) = stdin {
        cmd.write_stdin(stdin);
    }

    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("json output")
}
