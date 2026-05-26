use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};

use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::input::{DiscoveredInput, InputFormat, InputOptions, input_label, is_jsonl_candidate};
use crate::paths::ScanError;

const REPORT_SCHEMA: &str = "jscan.find.v1";

#[derive(Clone, Debug)]
pub struct FindOptions {
    pub predicates: Vec<FindPredicate>,
    pub mode: MatchMode,
    pub record_root: Option<PathExpr>,
    pub match_limit: usize,
}

#[derive(Clone, Debug)]
pub enum FindPredicate {
    Has(PathExpr),
    Missing(PathExpr),
    Eq { path: PathExpr, value: String },
    Contains { path: PathExpr, value: String },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MatchMode {
    All,
    Any,
}

#[derive(Clone, Debug)]
pub struct PathExpr {
    raw: String,
    segments: Vec<PathSegment>,
}

#[derive(Clone, Debug)]
enum PathSegment {
    Field(String),
    ArrayItem,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindReport {
    pub schema: &'static str,
    pub partial: bool,
    pub error_count: usize,
    pub errors_truncated: bool,
    pub scanned_records: usize,
    pub matched_records: usize,
    pub predicates: Vec<FindPredicateReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<FindMatch>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindPredicateReport {
    pub predicate: String,
    pub matched_records: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindMatch {
    pub source: String,
    pub record: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub value: Value,
}

struct FindState<'a> {
    predicates: &'a [FindPredicate],
    mode: MatchMode,
    record_root: Option<&'a PathExpr>,
    match_limit: usize,
    predicate_matches: Vec<usize>,
    matches: Vec<FindMatch>,
    scanned_records: usize,
    matched_records: usize,
    errors: Vec<ScanError>,
    error_count: usize,
    max_errors: usize,
}

pub fn parse_path_expr(input: &str) -> Result<PathExpr> {
    let raw = input.trim();
    if raw.is_empty() {
        bail!("path expression cannot be empty");
    }

    let mut index = 0;
    let mut segments = Vec::new();
    if raw[index..].starts_with('$') {
        index += '$'.len_utf8();
    }

    while index < raw.len() {
        let rest = &raw[index..];
        if rest.starts_with('.') {
            index += '.'.len_utf8();
            let start = index;
            while index < raw.len() {
                let next = raw[index..].chars().next().expect("char");
                if matches!(next, '.' | '[') {
                    break;
                }
                index += next.len_utf8();
            }
            if start == index {
                bail!("empty field segment in path expression: {raw}");
            }
            segments.push(PathSegment::Field(raw[start..index].to_string()));
            continue;
        }

        if rest.starts_with("[]") {
            segments.push(PathSegment::ArrayItem);
            index += 2;
            continue;
        }

        if rest.starts_with('[') {
            let close = raw[index..]
                .find(']')
                .map(|offset| index + offset)
                .ok_or_else(|| anyhow::anyhow!("unclosed bracket in path expression: {raw}"))?;
            let inner = &raw[index + 1..close];
            if inner.is_empty() {
                segments.push(PathSegment::ArrayItem);
            } else {
                let field = serde_json::from_str::<String>(inner).map_err(|_| {
                    anyhow::anyhow!("bracket path segments must be JSON strings: {raw}")
                })?;
                segments.push(PathSegment::Field(field));
            }
            index = close + 1;
            continue;
        }

        if segments.is_empty() && !raw.starts_with('$') {
            let start = index;
            while index < raw.len() {
                let next = raw[index..].chars().next().expect("char");
                if matches!(next, '.' | '[') {
                    break;
                }
                index += next.len_utf8();
            }
            segments.push(PathSegment::Field(raw[start..index].to_string()));
            continue;
        }

        bail!("unsupported path expression near {:?}: {raw}", rest);
    }

    Ok(PathExpr {
        raw: raw.to_string(),
        segments,
    })
}

pub fn collect_find(
    inputs: &[DiscoveredInput],
    input_options: &InputOptions,
    options: &FindOptions,
) -> Result<FindReport> {
    let mut state = FindState {
        predicates: &options.predicates,
        mode: options.mode,
        record_root: options.record_root.as_ref(),
        match_limit: options.match_limit,
        predicate_matches: vec![0; options.predicates.len()],
        matches: Vec::new(),
        scanned_records: 0,
        matched_records: 0,
        errors: Vec::new(),
        error_count: 0,
        max_errors: input_options.max_errors,
    };

    for input in inputs {
        let source = input_label(input);
        let format = effective_format(input, input_options.format);
        process_input(&mut state, input, &source, format);
    }

    Ok(state.finish())
}

fn effective_format(input: &DiscoveredInput, requested_format: InputFormat) -> InputFormat {
    match requested_format {
        InputFormat::Auto if is_jsonl_candidate(input) => InputFormat::Jsonl,
        format => format,
    }
}

fn process_input(
    state: &mut FindState<'_>,
    input: &DiscoveredInput,
    source: &str,
    format: InputFormat,
) {
    match format {
        InputFormat::Json => process_json_document(state, input, source),
        InputFormat::Jsonl => process_jsonl_input(state, input, source),
        InputFormat::Auto => process_auto_input(state, input, source),
    }
}

fn process_json_document(state: &mut FindState<'_>, input: &DiscoveredInput, source: &str) {
    let Some(contents) = read_to_string(state, input, source) else {
        return;
    };
    process_json_contents(state, source, &contents);
}

fn process_auto_input(state: &mut FindState<'_>, input: &DiscoveredInput, source: &str) {
    let Some(contents) = read_to_string(state, input, source) else {
        return;
    };

    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(&contents) {
        Ok(value) => process_document_value(state, source, None, &value),
        Err(error) if looks_line_delimited(&contents) => {
            let records = process_jsonl_contents(state, source, &contents);
            if records == 0 {
                state.push_error(ScanError {
                    source: source.to_string(),
                    line: Some(error.line()),
                    message: error.to_string(),
                });
            }
        }
        Err(error) => {
            state.push_error(ScanError {
                source: source.to_string(),
                line: Some(error.line()),
                message: error.to_string(),
            });
        }
    }
}

fn process_json_contents(state: &mut FindState<'_>, source: &str, contents: &str) {
    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(contents) {
        Ok(value) => process_document_value(state, source, None, &value),
        Err(error) => {
            state.push_error(ScanError {
                source: source.to_string(),
                line: Some(error.line()),
                message: error.to_string(),
            });
        }
    }
}

fn read_to_string(
    state: &mut FindState<'_>,
    input: &DiscoveredInput,
    source: &str,
) -> Option<String> {
    let result = match input {
        DiscoveredInput::Stdin => {
            let mut contents = String::new();
            io::stdin().read_to_string(&mut contents).map(|_| contents)
        }
        DiscoveredInput::File(path) => fs::read_to_string(path),
    };

    match result {
        Ok(contents) => Some(contents),
        Err(error) => {
            state.push_error(ScanError {
                source: source.to_string(),
                line: None,
                message: format!("could not read input: {error}"),
            });
            None
        }
    }
}

fn process_jsonl_input(state: &mut FindState<'_>, input: &DiscoveredInput, source: &str) {
    match input {
        DiscoveredInput::Stdin => {
            let stdin = io::stdin();
            process_jsonl_reader(state, source, stdin.lock());
        }
        DiscoveredInput::File(path) => match File::open(path) {
            Ok(file) => process_jsonl_reader(state, source, BufReader::new(file)),
            Err(error) => {
                state.push_error(ScanError {
                    source: source.to_string(),
                    line: None,
                    message: format!("could not read input: {error}"),
                });
            }
        },
    }
}

fn process_jsonl_reader<R: BufRead>(state: &mut FindState<'_>, source: &str, mut reader: R) {
    let mut line = String::new();
    let mut line_number = 0;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                line_number += 1;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(trimmed) {
                    Ok(value) => {
                        process_record_rooted_value(state, source, Some(line_number), &value)
                    }
                    Err(error) => state.push_error(ScanError {
                        source: source.to_string(),
                        line: Some(line_number),
                        message: error.to_string(),
                    }),
                }
            }
            Err(error) => {
                state.push_error(ScanError {
                    source: source.to_string(),
                    line: Some(line_number + 1),
                    message: format!("could not read line: {error}"),
                });
                break;
            }
        }
    }
}

fn process_jsonl_contents(state: &mut FindState<'_>, source: &str, contents: &str) -> usize {
    let mut records = 0;

    for (line_index, line) in contents.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => {
                records += 1;
                process_record_rooted_value(state, source, Some(line_number), &value);
            }
            Err(error) => state.push_error(ScanError {
                source: source.to_string(),
                line: Some(line_number),
                message: error.to_string(),
            }),
        }
    }

    records
}

fn process_document_value(
    state: &mut FindState<'_>,
    source: &str,
    line: Option<usize>,
    value: &Value,
) {
    if state.record_root.is_some() {
        process_record_rooted_value(state, source, line, value);
        return;
    }

    if let Value::Array(values) = value {
        for value in values {
            state.scan_record(source, line, value);
        }
    } else {
        state.scan_record(source, line, value);
    }
}

fn process_record_rooted_value(
    state: &mut FindState<'_>,
    source: &str,
    line: Option<usize>,
    value: &Value,
) {
    if let Some(record_root) = state.record_root {
        let mut values = Vec::new();
        values_at_path(value, &record_root.segments, &mut values);
        for value in values {
            state.scan_record(source, line, value);
        }
    } else {
        state.scan_record(source, line, value);
    }
}

fn looks_line_delimited(contents: &str) -> bool {
    let mut non_empty_lines = 0;
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        non_empty_lines += 1;
        if matches!(line, "{" | "[" | "}" | "]") {
            return false;
        }
        if non_empty_lines > 1 {
            return true;
        }
    }
    false
}

impl FindState<'_> {
    fn scan_record(&mut self, source: &str, line: Option<usize>, value: &Value) {
        self.scanned_records += 1;
        let record = self.scanned_records;
        let mut predicate_results = Vec::with_capacity(self.predicates.len());

        for predicate in self.predicates {
            predicate_results.push(predicate.matches(value));
        }

        for (index, matched) in predicate_results.iter().enumerate() {
            if *matched {
                self.predicate_matches[index] += 1;
            }
        }

        let record_matched = match self.mode {
            MatchMode::All => predicate_results.iter().all(|matched| *matched),
            MatchMode::Any => predicate_results.iter().any(|matched| *matched),
        };

        if record_matched {
            self.matched_records += 1;
            if self.matches.len() < self.match_limit {
                self.matches.push(FindMatch {
                    source: source.to_string(),
                    record,
                    line,
                    value: value.clone(),
                });
            }
        }
    }

    fn finish(self) -> FindReport {
        let predicates = self
            .predicates
            .iter()
            .zip(self.predicate_matches)
            .map(|(predicate, matched_records)| FindPredicateReport {
                predicate: predicate.display(),
                matched_records,
            })
            .collect();

        FindReport {
            schema: REPORT_SCHEMA,
            partial: self.error_count > 0,
            error_count: self.error_count,
            errors_truncated: self.error_count > self.errors.len(),
            scanned_records: self.scanned_records,
            matched_records: self.matched_records,
            predicates,
            matches: self.matches,
            errors: self.errors,
        }
    }

    fn push_error(&mut self, error: ScanError) {
        self.error_count += 1;
        if self.errors.len() < self.max_errors {
            self.errors.push(error);
        }
    }
}

impl FindPredicate {
    fn matches(&self, value: &Value) -> bool {
        let mut values = Vec::new();
        match self {
            FindPredicate::Has(path) => {
                values_at_path(value, &path.segments, &mut values);
                !values.is_empty()
            }
            FindPredicate::Missing(path) => {
                values_at_path(value, &path.segments, &mut values);
                values.is_empty()
            }
            FindPredicate::Eq {
                path,
                value: expected,
            } => {
                values_at_path(value, &path.segments, &mut values);
                values.iter().any(|value| value_equals(value, expected))
            }
            FindPredicate::Contains {
                path,
                value: expected,
            } => {
                values_at_path(value, &path.segments, &mut values);
                values.iter().any(|value| value_contains(value, expected))
            }
        }
    }

    fn display(&self) -> String {
        match self {
            FindPredicate::Has(path) => format!("has {}", path.raw),
            FindPredicate::Missing(path) => format!("missing {}", path.raw),
            FindPredicate::Eq { path, value } => format!("eq {} {}", path.raw, value),
            FindPredicate::Contains { path, value } => {
                format!("contains {} {}", path.raw, value)
            }
        }
    }
}

fn values_at_path<'a>(value: &'a Value, segments: &[PathSegment], output: &mut Vec<&'a Value>) {
    if segments.is_empty() {
        output.push(value);
        return;
    }

    match &segments[0] {
        PathSegment::Field(name) => {
            if let Value::Object(object) = value
                && let Some(value) = object.get(name)
            {
                values_at_path(value, &segments[1..], output);
            }
        }
        PathSegment::ArrayItem => {
            if let Value::Array(values) = value {
                for value in values {
                    values_at_path(value, &segments[1..], output);
                }
            }
        }
    }
}

fn value_equals(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(value) => value == expected,
        _ => serde_json::from_str::<Value>(expected)
            .map(|expected| value == &expected)
            .unwrap_or(false),
    }
}

fn value_contains(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(value) => value.contains(expected),
        Value::Array(values) => values.iter().any(|value| value_equals(value, expected)),
        _ => value_equals(value, expected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dot_and_array_paths() {
        let path = parse_path_expr("$.result.domainNames[]").expect("path");

        assert_eq!(path.segments.len(), 3);
    }

    #[test]
    fn predicates_match_nested_values() {
        let value = serde_json::json!({
            "result": {
                "status": "timeout",
                "domainNames": ["dev.azure.com", "example.test"]
            }
        });
        let predicate = FindPredicate::Contains {
            path: parse_path_expr("$.result.domainNames").expect("path"),
            value: "dev.azure.com".to_string(),
        };

        assert!(predicate.matches(&value));
    }
}
