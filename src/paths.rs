use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::input::{
    ContentClassification, DiscoveredInput, InputFormat, InputOptions, classify_auto_contents,
    input_label, resolve_input_format,
};

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
    types: TypeCounts,
    samples: Vec<PathSample>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TypeCounts {
    null: usize,
    boolean: usize,
    integer: usize,
    number: usize,
    string: usize,
    array: usize,
    object: usize,
}

pub fn collect_paths(
    inputs: &[DiscoveredInput],
    input_options: &InputOptions,
    paths_options: &PathsOptions,
) -> Result<PathReport> {
    let mut state = CollectorState::new(
        input_options.max_errors,
        paths_options.samples_per_path,
        paths_options.sample_max_chars,
    );

    for input in inputs {
        let label = input_label(input);
        let format = resolve_input_format(input, input_options.format);
        let mut source_state = SourceState::new(label, format);
        let before_errors = state.error_count;
        process_input(&mut state, &mut source_state, input);
        let errors = state.error_count - before_errors;

        state.sources.push(source_state.finish(errors));
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

fn process_auto_input(
    state: &mut CollectorState,
    source_state: &mut SourceState,
    input: &DiscoveredInput,
    source: &str,
) {
    let Some(contents) = read_to_string(state, input, source) else {
        return;
    };

    match classify_auto_contents(&contents) {
        ContentClassification::Empty => {}
        ContentClassification::Json(value) => {
            source_state.set_format(InputFormat::Json);
            source_state.record(&value);
            let root = state.root_node();
            visit_value(state, source, 1, Some(1), root, &value);
        }
        ContentClassification::Jsonl => {
            let buffered = parse_jsonl_buffer(source, &contents, state.max_errors);
            if buffered.records.is_empty() {
                state.push_error(ScanError {
                    source: source.to_string(),
                    line: None,
                    message: "input looked line-delimited but no JSON records parsed".to_string(),
                });
                return;
            }

            source_state.set_format(InputFormat::Jsonl);
            apply_jsonl_buffer(state, source_state, source, buffered);
        }
        ContentClassification::InvalidJson { line, message } => {
            state.push_error(ScanError {
                source: source.to_string(),
                line: Some(line),
                message,
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
            let root = state.root_node();
            visit_value(state, source, 1, Some(1), root, &value);
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
                        let root = state.root_node();
                        visit_value(state, source, records, Some(line_number), root, &value);
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
        let root = state.root_node();
        visit_value(
            state,
            source,
            records,
            Some(record.line),
            root,
            &record.value,
        );
    }
}

struct CollectorState {
    paths: PathTrie,
    sources: Vec<SourceReport>,
    errors: Vec<ScanError>,
    error_count: usize,
    max_errors: usize,
    samples_per_path: usize,
    sample_max_chars: usize,
}

#[derive(Debug)]
struct PathTrie {
    nodes: Vec<PathNode>,
}

#[derive(Debug)]
struct PathNode {
    segment: Option<PathSegment>,
    parent: Option<usize>,
    children: Vec<usize>,
    entry: MutablePathEntry,
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

impl TypeCounts {
    fn record(&mut self, value: &Value) {
        match value {
            Value::Null => self.null += 1,
            Value::Bool(_) => self.boolean += 1,
            Value::Number(number) if number.is_i64() || number.is_u64() => self.integer += 1,
            Value::Number(_) => self.number += 1,
            Value::String(_) => self.string += 1,
            Value::Array(_) => self.array += 1,
            Value::Object(_) => self.object += 1,
        }
    }

    fn into_map(self) -> BTreeMap<String, usize> {
        let mut types = BTreeMap::new();
        push_type_count(&mut types, "array", self.array);
        push_type_count(&mut types, "boolean", self.boolean);
        push_type_count(&mut types, "integer", self.integer);
        push_type_count(&mut types, "null", self.null);
        push_type_count(&mut types, "number", self.number);
        push_type_count(&mut types, "object", self.object);
        push_type_count(&mut types, "string", self.string);
        types
    }
}

fn push_type_count(types: &mut BTreeMap<String, usize>, name: &str, count: usize) {
    if count > 0 {
        types.insert(name.to_string(), count);
    }
}

impl PathTrie {
    fn new() -> Self {
        Self {
            nodes: vec![PathNode {
                segment: None,
                parent: None,
                children: Vec::new(),
                entry: MutablePathEntry::default(),
            }],
        }
    }

    fn root(&self) -> usize {
        0
    }

    fn child_for_array_item(&mut self, parent: usize) -> usize {
        for index in 0..self.nodes[parent].children.len() {
            let child = self.nodes[parent].children[index];
            if matches!(self.nodes[child].segment, Some(PathSegment::ArrayItem)) {
                return child;
            }
        }

        self.push_child(parent, PathSegment::ArrayItem)
    }

    fn child_for_field(&mut self, parent: usize, key: &str) -> usize {
        for index in 0..self.nodes[parent].children.len() {
            let child = self.nodes[parent].children[index];
            if let Some(PathSegment::Field { name }) = &self.nodes[child].segment
                && name == key
            {
                return child;
            }
        }

        self.push_child(
            parent,
            PathSegment::Field {
                name: key.to_string(),
            },
        )
    }

    fn push_child(&mut self, parent: usize, segment: PathSegment) -> usize {
        let node = self.nodes.len();
        self.nodes.push(PathNode {
            segment: Some(segment),
            parent: Some(parent),
            children: Vec::new(),
            entry: MutablePathEntry::default(),
        });
        self.nodes[parent].children.push(node);
        node
    }

    fn path_entries(self) -> Vec<PathEntry> {
        let segments_by_node = (0..self.nodes.len())
            .map(|node| self.segments_for_node(node))
            .collect::<Vec<_>>();
        let mut entries = self
            .nodes
            .into_iter()
            .zip(segments_by_node)
            .filter(|(node, _)| node.entry.count > 0)
            .map(|(node, segments)| PathEntry {
                display_path: display_path(&segments),
                pointer_template: pointer_template(&segments),
                segments,
                count: node.entry.count,
                types: node.entry.types.into_map(),
                samples: node.entry.samples,
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| left.segments.cmp(&right.segments));
        entries
    }

    fn segments_for_node(&self, mut node: usize) -> Vec<PathSegment> {
        let mut segments = Vec::new();
        while let Some(parent) = self.nodes[node].parent {
            let segment = self.nodes[node]
                .segment
                .as_ref()
                .expect("non-root path node has a segment");
            segments.push(segment.clone());
            node = parent;
        }
        segments.reverse();
        segments
    }
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
        increment_string_count(&mut self.root_kinds, json_type(value));

        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    increment_string_count(&mut self.top_level_fields, key);
                    if let Value::Array(items) = value {
                        let field = if self.array_fields.contains_key(key.as_str()) {
                            self.array_fields
                                .get_mut(key.as_str())
                                .expect("array field exists")
                        } else {
                            self.array_fields
                                .insert(key.clone(), SourceArrayFieldState::default());
                            self.array_fields
                                .get_mut(key.as_str())
                                .expect("array field was inserted")
                        };
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

fn increment_string_count(counts: &mut BTreeMap<String, usize>, key: &str) {
    if let Some(count) = counts.get_mut(key) {
        *count += 1;
    } else {
        counts.insert(key.to_string(), 1);
    }
}

impl CollectorState {
    fn new(max_errors: usize, samples_per_path: usize, sample_max_chars: usize) -> Self {
        Self {
            paths: PathTrie::new(),
            sources: Vec::new(),
            errors: Vec::new(),
            error_count: 0,
            max_errors,
            samples_per_path,
            sample_max_chars,
        }
    }

    fn root_node(&self) -> usize {
        self.paths.root()
    }

    fn finish(self) -> PathReport {
        let paths = self.paths.path_entries();

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

fn visit_value(
    state: &mut CollectorState,
    source: &str,
    record: usize,
    line: Option<usize>,
    node: usize,
    value: &Value,
) {
    record_path(state, source, record, line, node, value);

    match value {
        Value::Array(values) => {
            let child = state.paths.child_for_array_item(node);
            for value in values {
                visit_value(state, source, record, line, child, value);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                let child = state.paths.child_for_field(node, key);
                visit_value(state, source, record, line, child, value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn record_path(
    state: &mut CollectorState,
    source: &str,
    record: usize,
    line: Option<usize>,
    node: usize,
    value: &Value,
) {
    let entry = &mut state.paths.nodes[node].entry;
    entry.count += 1;
    entry.types.record(value);

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
        let mut state = CollectorState::new(10, 2, 200);

        let root = state.root_node();
        visit_value(&mut state, "fixture", 1, Some(1), root, &value);
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
