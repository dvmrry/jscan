use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::input::{DiscoveredInput, InputFormat, InputOptions, input_label, is_jsonl_candidate};

const REPORT_SCHEMA: &str = "jscan.paths.v1";

#[derive(Clone, Debug)]
pub struct PathsOptions {
    pub samples_per_path: usize,
    pub sample_max_chars: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathReport {
    pub schema: &'static str,
    pub partial: bool,
    pub error_count: usize,
    pub errors_truncated: bool,
    pub sources: Vec<SourceReport>,
    pub paths: Vec<PathEntry>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug)]
pub struct PathListReport {
    pub partial: bool,
    pub error_count: usize,
    pub errors_truncated: bool,
    pub paths: Vec<String>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceReport {
    pub source: String,
    pub records: usize,
    pub errors: usize,
    pub format: InputFormat,
    pub root_kinds: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub top_level_fields: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "is_zero")]
    pub top_level_array_items: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub array_fields: Vec<SourceArrayField>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceArrayField {
    pub name: String,
    pub count: usize,
    pub item_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathEntry {
    pub display_path: String,
    pub pointer_template: String,
    pub segments: Vec<PathSegment>,
    pub count: usize,
    pub types: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<PathSample>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathSegment {
    Field { name: String },
    ArrayItem,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathSample {
    pub source: String,
    pub record: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub value_preview: Value,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanError {
    pub source: String,
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Debug, Default)]
struct MutablePathEntry {
    count: usize,
    types: BTreeMap<String, usize>,
    samples: Vec<PathSample>,
}

pub fn collect_paths(
    inputs: &[DiscoveredInput],
    input_options: &InputOptions,
    paths_options: &PathsOptions,
) -> Result<PathReport> {
    let mut state = CollectorState {
        paths: BTreeMap::new(),
        sources: Vec::new(),
        errors: Vec::new(),
        error_count: 0,
        max_errors: input_options.max_errors,
        samples_per_path: paths_options.samples_per_path,
        sample_max_chars: paths_options.sample_max_chars,
    };

    for input in inputs {
        let label = input_label(input);
        let format = effective_format(input, input_options.format);
        let mut source_state = SourceState::new(label, format);
        let before_errors = state.error_count;
        process_input(&mut state, &mut source_state, input);
        let errors = state.error_count - before_errors;

        state.sources.push(source_state.finish(errors));
    }

    Ok(state.finish())
}

pub fn collect_path_list(
    inputs: &[DiscoveredInput],
    input_options: &InputOptions,
) -> Result<PathListReport> {
    let mut state = PathListState {
        paths: HashSet::new(),
        errors: Vec::new(),
        error_count: 0,
        max_errors: input_options.max_errors,
    };

    for input in inputs {
        let label = input_label(input);
        let format = effective_format(input, input_options.format);
        process_path_list_input(&mut state, input, &label, format);
    }

    Ok(state.finish())
}

fn process_input(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    input: &DiscoveredInput,
) {
    let source = source_state.source.clone();
    match source_state.format {
        InputFormat::Json => process_json_document(state, source_state, input, &source),
        InputFormat::Jsonl => process_jsonl_input(state, source_state, input, &source),
        InputFormat::Auto => process_auto_input(state, source_state, input, &source),
    }
}

fn process_path_list_input(
    state: &mut PathListState,
    input: &DiscoveredInput,
    source: &str,
    format: InputFormat,
) {
    match format {
        InputFormat::Json => process_path_list_json_document(state, input, source),
        InputFormat::Jsonl => process_path_list_jsonl_input(state, input, source),
        InputFormat::Auto => process_path_list_auto_input(state, input, source),
    }
}

fn effective_format(input: &DiscoveredInput, requested_format: InputFormat) -> InputFormat {
    match requested_format {
        InputFormat::Auto if is_jsonl_candidate(input) => InputFormat::Jsonl,
        format => format,
    }
}

fn process_json_document(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    input: &DiscoveredInput,
    source: &str,
) {
    let Some(contents) = read_to_string(state, input, source) else {
        return;
    };
    process_json_contents(state, source_state, source, &contents);
}

fn process_path_list_json_document(
    state: &mut PathListState,
    input: &DiscoveredInput,
    source: &str,
) {
    let Some(contents) = read_to_string_for_path_list(state, input, source) else {
        return;
    };
    process_path_list_json_contents(state, source, &contents);
}

fn process_auto_input(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    input: &DiscoveredInput,
    source: &str,
) {
    let Some(contents) = read_to_string(state, input, source) else {
        return;
    };

    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(&contents) {
        Ok(value) => {
            source_state.set_format(InputFormat::Json);
            source_state.record(&value);
            visit_value(state, source, 1, Some(1), &mut Vec::new(), &value);
        }
        Err(error) if looks_line_delimited(&contents) => {
            let buffered = parse_jsonl_buffer(source, &contents, state.max_errors);
            if buffered.records.is_empty() {
                state.push_error(ScanError {
                    source: source.to_string(),
                    line: Some(error.line()),
                    message: error.to_string(),
                });
                return;
            }

            source_state.set_format(InputFormat::Jsonl);
            apply_jsonl_buffer(state, source_state, source, buffered);
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

fn process_path_list_auto_input(state: &mut PathListState, input: &DiscoveredInput, source: &str) {
    let Some(contents) = read_to_string_for_path_list(state, input, source) else {
        return;
    };

    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(&contents) {
        Ok(value) => visit_path_list_root(state, &value),
        Err(error) if looks_line_delimited(&contents) => {
            let records = process_path_list_jsonl_contents(state, source, &contents);
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

fn process_json_contents(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    source: &str,
    contents: &str,
) {
    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(contents) {
        Ok(value) => {
            source_state.record(&value);
            visit_value(state, source, 1, Some(1), &mut Vec::new(), &value);
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

fn process_path_list_json_contents(state: &mut PathListState, source: &str, contents: &str) {
    if contents.trim().is_empty() {
        return;
    }

    match serde_json::from_str::<Value>(contents) {
        Ok(value) => visit_path_list_root(state, &value),
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
    state: &mut CollectorState,
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

fn read_to_string_for_path_list(
    state: &mut PathListState,
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

fn process_jsonl_input(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    input: &DiscoveredInput,
    source: &str,
) {
    match input {
        DiscoveredInput::Stdin => {
            let stdin = io::stdin();
            process_jsonl_reader(state, source_state, source, stdin.lock());
        }
        DiscoveredInput::File(path) => match File::open(path) {
            Ok(file) => process_jsonl_reader(state, source_state, source, BufReader::new(file)),
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

fn process_path_list_jsonl_input(state: &mut PathListState, input: &DiscoveredInput, source: &str) {
    match input {
        DiscoveredInput::Stdin => {
            let stdin = io::stdin();
            process_path_list_jsonl_reader(state, source, stdin.lock());
        }
        DiscoveredInput::File(path) => match File::open(path) {
            Ok(file) => process_path_list_jsonl_reader(state, source, BufReader::new(file)),
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

fn process_path_list_jsonl_reader<R: BufRead>(
    state: &mut PathListState,
    source: &str,
    mut reader: R,
) {
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
                    Ok(value) => visit_path_list_root(state, &value),
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

fn process_jsonl_reader<R: BufRead>(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    source: &str,
    mut reader: R,
) {
    let mut line = String::new();
    let mut line_number = 0;
    let mut records = 0;

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
                        records += 1;
                        source_state.record(&value);
                        visit_value(
                            state,
                            source,
                            records,
                            Some(line_number),
                            &mut Vec::new(),
                            &value,
                        );
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

struct BufferedJsonl {
    records: Vec<BufferedRecord>,
    errors: Vec<ScanError>,
    error_count: usize,
}

struct BufferedRecord {
    line: usize,
    value: Value,
}

fn parse_jsonl_buffer(source: &str, contents: &str, max_errors: usize) -> BufferedJsonl {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut error_count = 0;

    for (line_index, line) in contents.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => records.push(BufferedRecord {
                line: line_number,
                value,
            }),
            Err(error) => {
                error_count += 1;
                if errors.len() < max_errors {
                    errors.push(ScanError {
                        source: source.to_string(),
                        line: Some(line_number),
                        message: error.to_string(),
                    });
                }
            }
        }
    }

    BufferedJsonl {
        records,
        errors,
        error_count,
    }
}

fn process_path_list_jsonl_contents(
    state: &mut PathListState,
    source: &str,
    contents: &str,
) -> usize {
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
                visit_path_list_root(state, &value);
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

fn apply_jsonl_buffer(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    source: &str,
    buffered: BufferedJsonl,
) {
    state.push_buffered_errors(buffered.errors, buffered.error_count);

    let mut records = 0;
    for record in buffered.records {
        records += 1;
        source_state.record(&record.value);
        visit_value(
            state,
            source,
            records,
            Some(record.line),
            &mut Vec::new(),
            &record.value,
        );
    }
}

fn looks_line_delimited(contents: &str) -> bool {
    let lines = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    lines.len() > 1
        && !lines
            .iter()
            .any(|line| matches!(*line, "{" | "[" | "}" | "]"))
}

struct CollectorState {
    paths: BTreeMap<Vec<PathSegment>, MutablePathEntry>,
    sources: Vec<SourceReport>,
    errors: Vec<ScanError>,
    error_count: usize,
    max_errors: usize,
    samples_per_path: usize,
    sample_max_chars: usize,
}

struct PathListState {
    paths: HashSet<String>,
    errors: Vec<ScanError>,
    error_count: usize,
    max_errors: usize,
}

#[derive(Debug)]
struct SourceState {
    source: String,
    format: InputFormat,
    records: usize,
    root_kinds: BTreeMap<String, usize>,
    top_level_fields: BTreeMap<String, usize>,
    top_level_array_items: usize,
    array_fields: BTreeMap<String, SourceArrayFieldState>,
}

#[derive(Debug, Default)]
struct SourceArrayFieldState {
    count: usize,
    item_count: usize,
}

impl SourceState {
    fn new(source: String, format: InputFormat) -> Self {
        Self {
            source,
            format,
            records: 0,
            root_kinds: BTreeMap::new(),
            top_level_fields: BTreeMap::new(),
            top_level_array_items: 0,
            array_fields: BTreeMap::new(),
        }
    }

    fn set_format(&mut self, format: InputFormat) {
        self.format = format;
    }

    fn record(&mut self, value: &Value) {
        self.records += 1;
        *self
            .root_kinds
            .entry(json_type(value).to_string())
            .or_insert(0) += 1;

        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    *self.top_level_fields.entry(key.clone()).or_insert(0) += 1;
                    if let Value::Array(items) = value {
                        let field = self.array_fields.entry(key.clone()).or_default();
                        field.count += 1;
                        field.item_count += items.len();
                    }
                }
            }
            Value::Array(items) => {
                self.top_level_array_items += items.len();
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    fn finish(self, errors: usize) -> SourceReport {
        let array_fields = self
            .array_fields
            .into_iter()
            .map(|(name, field)| SourceArrayField {
                name,
                count: field.count,
                item_count: field.item_count,
            })
            .collect();

        SourceReport {
            source: self.source,
            records: self.records,
            errors,
            format: self.format,
            root_kinds: self.root_kinds,
            top_level_fields: self.top_level_fields,
            top_level_array_items: self.top_level_array_items,
            array_fields,
        }
    }
}

impl CollectorState {
    fn finish(self) -> PathReport {
        let paths = self
            .paths
            .into_iter()
            .map(|(segments, entry)| PathEntry {
                display_path: display_path(&segments),
                pointer_template: pointer_template(&segments),
                segments,
                count: entry.count,
                types: entry.types,
                samples: entry.samples,
            })
            .collect();

        PathReport {
            schema: REPORT_SCHEMA,
            partial: self.error_count > 0,
            error_count: self.error_count,
            errors_truncated: self.error_count > self.errors.len(),
            sources: self.sources,
            paths,
            errors: self.errors,
        }
    }

    fn push_error(&mut self, error: ScanError) {
        self.error_count += 1;
        if self.errors.len() < self.max_errors {
            self.errors.push(error);
        }
    }

    fn push_buffered_errors(&mut self, errors: Vec<ScanError>, error_count: usize) {
        self.error_count += error_count;
        let available = self.max_errors.saturating_sub(self.errors.len());
        self.errors.extend(errors.into_iter().take(available));
    }
}

impl PathListState {
    fn finish(self) -> PathListReport {
        let mut paths = self.paths.into_iter().collect::<Vec<_>>();
        paths.sort();

        PathListReport {
            partial: self.error_count > 0,
            error_count: self.error_count,
            errors_truncated: self.error_count > self.errors.len(),
            paths,
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

fn visit_value(
    state: &mut CollectorState,
    source: &str,
    record: usize,
    line: Option<usize>,
    path: &mut Vec<PathSegment>,
    value: &Value,
) {
    record_path(state, source, record, line, path, value);

    match value {
        Value::Array(values) => {
            path.push(PathSegment::ArrayItem);
            for value in values {
                visit_value(state, source, record, line, path, value);
            }
            path.pop();
        }
        Value::Object(object) => {
            for (key, value) in object {
                path.push(PathSegment::Field { name: key.clone() });
                visit_value(state, source, record, line, path, value);
                path.pop();
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn visit_path_list_root(state: &mut PathListState, value: &Value) {
    visit_path_list_value(state, &mut "$".to_string(), value);
}

fn visit_path_list_value(state: &mut PathListState, path: &mut String, value: &Value) {
    if !state.paths.contains(path.as_str()) {
        state.paths.insert(path.clone());
    }

    match value {
        Value::Array(values) => {
            let path_len = path.len();
            path.push_str("[]");
            for value in values {
                visit_path_list_value(state, path, value);
            }
            path.truncate(path_len);
        }
        Value::Object(object) => {
            for (key, value) in object {
                let path_len = path.len();
                push_display_field(path, key);
                visit_path_list_value(state, path, value);
                path.truncate(path_len);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn push_display_field(path: &mut String, key: &str) {
    if is_simple_key(key) {
        path.push('.');
        path.push_str(key);
    } else {
        path.push('[');
        path.push_str(&serde_json::to_string(key).expect("serializing path key"));
        path.push(']');
    }
}

fn record_path(
    state: &mut CollectorState,
    source: &str,
    record: usize,
    line: Option<usize>,
    path: &[PathSegment],
    value: &Value,
) {
    let entry = state.paths.entry(path.to_vec()).or_default();
    entry.count += 1;
    *entry.types.entry(json_type(value).to_string()).or_insert(0) += 1;

    if state.samples_per_path > 0
        && entry.samples.len() < state.samples_per_path
        && sampleable(value)
    {
        let (value_preview, truncated) = sample_preview(value, state.sample_max_chars);
        if !sample_seen(&entry.samples, &value_preview) {
            entry.samples.push(PathSample {
                source: source.to_string(),
                record,
                line,
                value_preview,
                truncated,
            });
        }
    }
}

fn sampleable(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn sample_preview(value: &Value, max_chars: usize) -> (Value, bool) {
    match value {
        Value::String(value) => {
            let mut preview = String::new();
            let mut chars = value.chars();
            for _ in 0..max_chars {
                match chars.next() {
                    Some(ch) => preview.push(ch),
                    None => return (Value::String(preview), false),
                }
            }

            if chars.next().is_some() {
                preview.push_str("...");
                (Value::String(preview), true)
            } else {
                (Value::String(preview), false)
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => (value.clone(), false),
        Value::Array(_) | Value::Object(_) => unreachable!("samples are scalar only"),
    }
}

fn sample_seen(samples: &[PathSample], value_preview: &Value) -> bool {
    samples
        .iter()
        .any(|sample| &sample.value_preview == value_preview)
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn display_path(segments: &[PathSegment]) -> String {
    if segments.is_empty() {
        return "$".to_string();
    }

    let mut output = "$".to_string();
    for segment in segments {
        match segment {
            PathSegment::Field { name } if is_simple_key(name) => {
                output.push('.');
                output.push_str(name);
            }
            PathSegment::Field { name } => {
                output.push('[');
                output.push_str(&serde_json::to_string(name).expect("serializing path key"));
                output.push(']');
            }
            PathSegment::ArrayItem => output.push_str("[]"),
        }
    }
    output
}

fn pointer_template(segments: &[PathSegment]) -> String {
    if segments.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for segment in segments {
        output.push('/');
        match segment {
            PathSegment::Field { name } => output.push_str(&escape_pointer(name)),
            PathSegment::ArrayItem => output.push('*'),
        }
    }
    output
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn is_simple_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch == '-' || ch.is_ascii_alphanumeric())
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn inventories_paths_through_arrays() {
        let value = serde_json::json!({
            "users": [
                {"id": 1, "email": "a@example.test"},
                {"id": 2, "email": null}
            ]
        });
        let mut state = CollectorState {
            paths: BTreeMap::new(),
            sources: Vec::new(),
            errors: Vec::new(),
            error_count: 0,
            max_errors: 10,
            samples_per_path: 2,
            sample_max_chars: 200,
        };

        visit_value(&mut state, "fixture", 1, Some(1), &mut Vec::new(), &value);
        let report = state.finish();
        let paths: BTreeSet<_> = report
            .paths
            .iter()
            .map(|entry| entry.display_path.as_str())
            .collect();

        assert!(paths.contains("$.users[]"));
        assert!(paths.contains("$.users[].id"));
        assert!(paths.contains("$.users[].email"));

        let email = report
            .paths
            .iter()
            .find(|entry| entry.display_path == "$.users[].email")
            .expect("email path");
        assert_eq!(email.types.get("string"), Some(&1));
        assert_eq!(email.types.get("null"), Some(&1));
    }

    #[test]
    fn records_source_top_level_summary() {
        let report = collect_paths(
            &[DiscoveredInput::File("tests/fixtures/basic.json".into())],
            &InputOptions {
                format: InputFormat::Auto,
                max_errors: 10,
            },
            &PathsOptions {
                samples_per_path: 0,
                sample_max_chars: 200,
            },
        )
        .expect("paths");
        let source = &report.sources[0];

        assert_eq!(source.records, 1);
        assert_eq!(source.root_kinds.get("object"), Some(&1));
        assert_eq!(source.top_level_fields.get("users"), Some(&1));
        assert_eq!(source.array_fields[0].name, "users");
        assert_eq!(source.array_fields[0].item_count, 2);
    }

    #[test]
    fn escapes_unusual_path_keys() {
        let segments = vec![
            PathSegment::Field {
                name: "a.b".to_string(),
            },
            PathSegment::Field {
                name: "slash/key".to_string(),
            },
        ];

        assert_eq!(display_path(&segments), "$[\"a.b\"][\"slash/key\"]");
        assert_eq!(pointer_template(&segments), "/a.b/slash~1key");
    }

    #[test]
    fn truncates_string_sample_previews() {
        let (preview, truncated) = sample_preview(&Value::String("abcdef".to_string()), 3);

        assert_eq!(preview, Value::String("abc...".to_string()));
        assert!(truncated);
    }
}
