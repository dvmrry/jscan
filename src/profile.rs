use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::paths::{PathEntry, PathReport, PathSample, ScanError, SourceReport};
use crate::shape::ShapeReport;

const REPORT_SCHEMA: &str = "jscan.profile.v1";
const DEFAULT_PATH_LIMIT: usize = 30;
const DEFAULT_SHAPE_LIMIT: usize = 30;
const DEFAULT_VARIATION_LIMIT: usize = 20;
const DEFAULT_COMMON_VALUE_LIMIT: usize = 20;
const DEFAULT_SAMPLE_LIMIT: usize = 20;

#[derive(Clone, Debug)]
pub struct ProfileOptions {
    pub budget_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfileReport {
    pub schema: &'static str,
    pub partial: bool,
    pub error_count: usize,
    pub errors_truncated: bool,
    pub budget: BudgetReport,
    pub sources: Vec<ProfileSource>,
    pub containers: Vec<ContainerFact>,
    pub record_roots: Vec<RecordRootFact>,
    pub path_facts: Vec<PathFact>,
    pub shape_facts: Vec<ShapeFact>,
    pub type_variations: Vec<TypeVariationFact>,
    pub common_values: Vec<CommonValueFact>,
    pub samples: Vec<ProfileSample>,
    pub next_tools: Vec<NextToolHint>,
    pub next_commands: Vec<NextCommandHint>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BudgetReport {
    pub requested_bytes: usize,
    pub estimated_bytes: usize,
    pub truncated: bool,
    pub omitted: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfileSource {
    pub source: String,
    pub records: usize,
    pub errors: usize,
    pub format: crate::InputFormat,
    pub root_kinds: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub top_level_fields: Vec<FieldCount>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub array_fields: Vec<ArrayFieldCount>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldCount {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArrayFieldCount {
    pub name: String,
    pub count: usize,
    pub item_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContainerFact {
    pub source: String,
    pub kind: String,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordRootFact {
    pub source: String,
    pub display_path: String,
    pub pointer_template: String,
    pub confidence: f64,
    pub reason: String,
    pub record_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathFact {
    pub display_path: String,
    pub pointer_template: String,
    pub count: usize,
    pub types: BTreeMap<String, usize>,
    pub under_record_root: bool,
    pub signals: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShapeFact {
    pub object_path: String,
    pub field: String,
    pub display_path: String,
    pub count: usize,
    pub presence: f64,
    pub required: bool,
    pub types: BTreeMap<String, usize>,
    pub signals: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TypeVariationFact {
    pub display_path: String,
    pub pointer_template: String,
    pub types: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<PathSample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommonValueFact {
    pub display_path: String,
    pub pointer_template: String,
    pub values: Vec<Value>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfileSample {
    pub source: String,
    pub record: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub path: String,
    pub value_preview: Value,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NextToolHint {
    pub tool: String,
    pub reason: String,
    pub caveat: String,
    pub command: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NextCommandHint {
    pub command: String,
    pub reason: String,
}

pub fn build_profile(
    path_report: &PathReport,
    shape_report: &ShapeReport,
    options: &ProfileOptions,
) -> Result<ProfileReport> {
    let profile_sources = profile_sources(&path_report.sources);
    let containers = detect_containers(&path_report.sources);
    let record_roots = detect_record_roots(&path_report.sources);
    let record_root_prefixes = record_roots
        .iter()
        .map(|root| root.pointer_template.as_str())
        .filter(|pointer| !pointer.is_empty())
        .collect::<Vec<_>>();
    let path_facts = ranked_path_facts(
        &path_report.paths,
        &record_root_prefixes,
        DEFAULT_PATH_LIMIT,
    );
    let shape_facts = ranked_shape_facts(shape_report, DEFAULT_SHAPE_LIMIT);
    let type_variations = type_variations(&path_report.paths, DEFAULT_VARIATION_LIMIT);
    let common_values = common_values(&path_report.paths, DEFAULT_COMMON_VALUE_LIMIT);
    let samples = profile_samples(&path_facts, &path_report.paths, DEFAULT_SAMPLE_LIMIT);

    let mut report = ProfileReport {
        schema: REPORT_SCHEMA,
        partial: path_report.partial,
        error_count: path_report.error_count,
        errors_truncated: path_report.errors_truncated,
        budget: BudgetReport {
            requested_bytes: options.budget_bytes,
            estimated_bytes: 0,
            truncated: false,
            omitted: Vec::new(),
        },
        sources: profile_sources,
        containers,
        record_roots,
        path_facts,
        shape_facts,
        type_variations,
        common_values,
        samples,
        next_tools: next_tools(),
        next_commands: next_commands(),
        errors: path_report.errors.clone(),
    };

    apply_budget(&mut report, options.budget_bytes)?;
    Ok(report)
}

fn profile_sources(sources: &[SourceReport]) -> Vec<ProfileSource> {
    sources
        .iter()
        .map(|source| ProfileSource {
            source: source.source.clone(),
            records: source.records,
            errors: source.errors,
            format: source.format,
            root_kinds: source.root_kinds.clone(),
            top_level_fields: top_counts(&source.top_level_fields, 20)
                .into_iter()
                .map(|(name, count)| FieldCount { name, count })
                .collect(),
            array_fields: source
                .array_fields
                .iter()
                .take(10)
                .map(|field| ArrayFieldCount {
                    name: field.name.clone(),
                    count: field.count,
                    item_count: field.item_count,
                })
                .collect(),
        })
        .collect()
}

fn detect_containers(sources: &[SourceReport]) -> Vec<ContainerFact> {
    let mut containers = Vec::new();
    for source in sources {
        if source.records == 0 && source.errors == 0 {
            containers.push(container(
                source,
                "empty",
                1.0,
                "no records and no parse errors",
            ));
            continue;
        }

        if source.format == crate::InputFormat::Jsonl {
            containers.push(container(
                source,
                "jsonl_records",
                0.95,
                "input parsed as JSONL / NDJSON records",
            ));
        }

        if has_fields(source, &["preview", "result"]) {
            containers.push(container(
                source,
                "splunk_preview_result_wrapper",
                0.95,
                "top-level records contain preview and result fields",
            ));
        } else if has_fields(source, &["result"]) {
            containers.push(container(
                source,
                "splunk_result_wrapper",
                0.90,
                "top-level records contain result field",
            ));
        }

        if has_fields(source, &["totalPages", "totalCount", "list"]) {
            containers.push(container(
                source,
                "paged_list_wrapper",
                0.90,
                "top-level object has totalPages, totalCount, and list fields",
            ));
        }

        if source.root_kinds.contains_key("array") {
            containers.push(container(
                source,
                "root_array",
                0.90,
                "top-level JSON value is an array",
            ));
        }

        if source.root_kinds.contains_key("object") {
            containers.push(container(
                source,
                "root_object",
                0.70,
                "top-level JSON value or JSONL records are objects",
            ));
        }

        if dominant_array_field(source).is_some() {
            containers.push(container(
                source,
                "dominant_array_field",
                0.75,
                "top-level object has an array field that likely contains records",
            ));
        }

        if containers.iter().all(|fact| fact.source != source.source) {
            containers.push(container(
                source,
                "unknown",
                0.20,
                "no known container pattern",
            ));
        }
    }
    containers
}

fn detect_record_roots(sources: &[SourceReport]) -> Vec<RecordRootFact> {
    let mut roots = Vec::new();
    for source in sources {
        if has_fields(source, &["result"]) {
            roots.push(record_root(
                source,
                "$.result",
                "/result",
                0.95,
                "Splunk-style result wrapper",
                source.records,
            ));
            continue;
        }

        if has_fields(source, &["totalPages", "totalCount", "list"]) {
            let item_count = source
                .array_fields
                .iter()
                .find(|field| field.name == "list")
                .map(|field| field.item_count)
                .unwrap_or(source.records);
            roots.push(record_root(
                source,
                "$.list[]",
                "/list/*",
                0.90,
                "paged API wrapper list field",
                item_count,
            ));
            continue;
        }

        if source.root_kinds.contains_key("array") {
            roots.push(record_root(
                source,
                "$[]",
                "/*",
                0.90,
                "top-level array items",
                source.top_level_array_items,
            ));
            continue;
        }

        if let Some(field) = dominant_array_field(source) {
            roots.push(record_root(
                source,
                &format!("$.{}[]", display_key(&field.name)),
                &format!("/{}/*", escape_pointer(&field.name)),
                0.75,
                "dominant top-level array field",
                field.item_count,
            ));
            continue;
        }

        if source.root_kinds.contains_key("object") {
            roots.push(record_root(
                source,
                "$",
                "",
                0.55,
                "top-level objects are the best record root guess",
                source.records,
            ));
        }
    }
    roots
}

fn ranked_path_facts(
    paths: &[PathEntry],
    record_root_prefixes: &[&str],
    limit: usize,
) -> Vec<PathFact> {
    let mut entries = paths
        .iter()
        .filter(|entry| entry.display_path != "$")
        .map(|entry| {
            let signals = path_signals(entry);
            let score = path_score(entry, &signals);
            (score, entry, signals)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.display_path.cmp(&b.1.display_path))
    });

    entries
        .into_iter()
        .take(limit)
        .map(|(_, entry, signals)| PathFact {
            display_path: entry.display_path.clone(),
            pointer_template: entry.pointer_template.clone(),
            count: entry.count,
            types: entry.types.clone(),
            under_record_root: under_record_root(&entry.pointer_template, record_root_prefixes),
            signals,
        })
        .collect()
}

fn ranked_shape_facts(shape_report: &ShapeReport, limit: usize) -> Vec<ShapeFact> {
    let mut facts = shape_report
        .objects
        .iter()
        .flat_map(|object| {
            object.fields.iter().map(move |field| {
                let signals = field_signals(&field.display_path, field.types.len() > 1);
                let score = field_score(field.count, field.presence, &signals, field.types.len());
                (score, object, field, signals)
            })
        })
        .collect::<Vec<_>>();
    facts.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.2.display_path.cmp(&b.2.display_path))
    });

    facts
        .into_iter()
        .take(limit)
        .map(|(_, object, field, signals)| ShapeFact {
            object_path: object.display_path.clone(),
            field: field.name.clone(),
            display_path: field.display_path.clone(),
            count: field.count,
            presence: field.presence,
            required: field.required,
            types: field.types.clone(),
            signals,
        })
        .collect()
}

fn type_variations(paths: &[PathEntry], limit: usize) -> Vec<TypeVariationFact> {
    paths
        .iter()
        .filter(|entry| entry.types.len() > 1)
        .take(limit)
        .map(|entry| TypeVariationFact {
            display_path: entry.display_path.clone(),
            pointer_template: entry.pointer_template.clone(),
            types: entry.types.clone(),
            samples: entry.samples.clone(),
        })
        .collect()
}

fn common_values(paths: &[PathEntry], limit: usize) -> Vec<CommonValueFact> {
    paths
        .iter()
        .filter(|entry| !entry.samples.is_empty())
        .filter(|entry| {
            entry.types.contains_key("string")
                || entry.types.contains_key("boolean")
                || entry.types.contains_key("integer")
        })
        .take(limit)
        .map(|entry| CommonValueFact {
            display_path: entry.display_path.clone(),
            pointer_template: entry.pointer_template.clone(),
            values: entry
                .samples
                .iter()
                .map(|sample| sample.value_preview.clone())
                .collect(),
            note: "sampled distinct values; counts are not tracked in v1".to_string(),
        })
        .collect()
}

fn profile_samples(
    path_facts: &[PathFact],
    paths: &[PathEntry],
    limit: usize,
) -> Vec<ProfileSample> {
    let mut samples = Vec::new();
    for fact in path_facts {
        let Some(entry) = paths
            .iter()
            .find(|entry| entry.display_path == fact.display_path)
        else {
            continue;
        };

        for sample in &entry.samples {
            samples.push(ProfileSample {
                source: sample.source.clone(),
                record: sample.record,
                line: sample.line,
                path: entry.display_path.clone(),
                value_preview: sample.value_preview.clone(),
                truncated: sample.truncated,
            });
            if samples.len() >= limit {
                return samples;
            }
        }
    }
    samples
}

fn apply_budget(report: &mut ProfileReport, budget_bytes: usize) -> Result<()> {
    refresh_estimate(report)?;
    if budget_bytes == 0 || report.budget.estimated_bytes <= budget_bytes {
        return Ok(());
    }

    if !report.samples.is_empty() {
        report.samples.clear();
        report.budget.omitted.push("samples".to_string());
    }
    if !report.common_values.is_empty() {
        report.common_values.clear();
        report.budget.omitted.push("common_values".to_string());
    }
    refresh_estimate(report)?;
    if report.budget.estimated_bytes <= budget_bytes {
        report.budget.truncated = true;
        return Ok(());
    }

    truncate_with_note(
        &mut report.path_facts,
        15,
        &mut report.budget.omitted,
        "path_facts",
    );
    truncate_with_note(
        &mut report.shape_facts,
        15,
        &mut report.budget.omitted,
        "shape_facts",
    );
    truncate_with_note(
        &mut report.type_variations,
        10,
        &mut report.budget.omitted,
        "type_variations",
    );
    report.budget.truncated = true;
    refresh_estimate(report)?;
    Ok(())
}

fn refresh_estimate(report: &mut ProfileReport) -> Result<()> {
    report.budget.estimated_bytes = 0;
    report.budget.estimated_bytes = serde_json::to_vec(report)?.len();
    Ok(())
}

fn truncate_with_note<T>(
    values: &mut Vec<T>,
    max_len: usize,
    omitted: &mut Vec<String>,
    label: &str,
) {
    if values.len() > max_len {
        values.truncate(max_len);
        omitted.push(format!("{label}_tail"));
    }
}

fn next_tools() -> Vec<NextToolHint> {
    vec![
        NextToolHint {
            tool: "rg".to_string(),
            reason: "fast raw smoke test for rare strings".to_string(),
            caveat: "counts text occurrences, not matching JSON objects".to_string(),
            command: "rg '<term>' <input>".to_string(),
        },
        NextToolHint {
            tool: "jaq".to_string(),
            reason: "fast JSONL-aware structural filtering and aggregation".to_string(),
            caveat: "requires a known filter and JSON-aware semantics".to_string(),
            command: "jaq -c '<filter>' <input>".to_string(),
        },
        NextToolHint {
            tool: "jq".to_string(),
            reason: "widely available JSON transformation and aggregation".to_string(),
            caveat: "can be slower for repeated broad probes".to_string(),
            command: "jq -c '<filter>' <input>".to_string(),
        },
        NextToolHint {
            tool: "jg".to_string(),
            reason: "fast JSON-aware field or path presence checks".to_string(),
            caveat: "does not replace jq/jaq for value predicates and aggregation".to_string(),
            command: "jg '<path-pattern>' <input>".to_string(),
        },
    ]
}

fn next_commands() -> Vec<NextCommandHint> {
    vec![
        NextCommandHint {
            command: "jscan paths <input> --samples 2 --json".to_string(),
            reason: "inspect full path/type inventory with small scalar samples".to_string(),
        },
        NextCommandHint {
            command: "jscan shape <input> --samples 2 --json".to_string(),
            reason: "inspect object fields, optionality, and array item shapes".to_string(),
        },
    ]
}

fn container(source: &SourceReport, kind: &str, confidence: f64, reason: &str) -> ContainerFact {
    ContainerFact {
        source: source.source.clone(),
        kind: kind.to_string(),
        confidence,
        reason: reason.to_string(),
    }
}

fn record_root(
    source: &SourceReport,
    display_path: &str,
    pointer_template: &str,
    confidence: f64,
    reason: &str,
    record_count: usize,
) -> RecordRootFact {
    RecordRootFact {
        source: source.source.clone(),
        display_path: display_path.to_string(),
        pointer_template: pointer_template.to_string(),
        confidence,
        reason: reason.to_string(),
        record_count,
    }
}

fn has_fields(source: &SourceReport, fields: &[&str]) -> bool {
    fields
        .iter()
        .all(|field| source.top_level_fields.contains_key(*field))
}

fn dominant_array_field(source: &SourceReport) -> Option<&crate::paths::SourceArrayField> {
    source
        .array_fields
        .iter()
        .max_by_key(|field| (field.item_count, field.count))
}

fn top_counts(counts: &BTreeMap<String, usize>, limit: usize) -> Vec<(String, usize)> {
    let mut counts = counts
        .iter()
        .map(|(name, count)| (name.clone(), *count))
        .collect::<Vec<_>>();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts.truncate(limit);
    counts
}

fn path_score(entry: &PathEntry, signals: &[String]) -> usize {
    entry.count
        + signals.len() * 1_000
        + usize::from(entry.types.len() > 1) * 2_000
        + usize::from(!entry.samples.is_empty()) * 200
}

fn field_score(count: usize, presence: f64, signals: &[String], type_count: usize) -> usize {
    count
        + signals.len() * 1_000
        + usize::from(type_count > 1) * 2_000
        + if presence < 1.0 { 500 } else { 0 }
}

fn path_signals(entry: &PathEntry) -> Vec<String> {
    field_signals(&entry.display_path, entry.types.len() > 1)
}

fn field_signals(path: &str, mixed_types: bool) -> Vec<String> {
    let mut signals = Vec::new();
    let lower = path.to_ascii_lowercase();
    if mixed_types {
        signals.push("mixed_types".to_string());
    }
    for keyword in [
        "time",
        "timestamp",
        "user",
        "host",
        "device",
        "action",
        "status",
        "error",
        "policy",
        "url",
        "domain",
        "ip",
        "source",
        "destination",
        "connector",
        "tunnel",
        "app",
    ] {
        if lower.contains(keyword) {
            signals.push(format!("keyword:{keyword}"));
        }
    }
    signals
}

fn under_record_root(pointer: &str, roots: &[&str]) -> bool {
    roots.iter().any(|root| pointer.starts_with(*root))
}

fn display_key(key: &str) -> String {
    if key
        .chars()
        .all(|ch| ch == '_' || ch == '-' || ch.is_ascii_alphanumeric())
    {
        key.to_string()
    } else {
        serde_json::to_string(key).expect("serializing display key")
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
