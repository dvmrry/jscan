use std::collections::BTreeMap;
use std::collections::BTreeSet;

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
    pub minimum_bytes_exceeded: bool,
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
    let common_values = common_values(&path_report.paths, &path_facts, DEFAULT_COMMON_VALUE_LIMIT);
    let samples = profile_samples(&path_facts, &path_report.paths, DEFAULT_SAMPLE_LIMIT);
    let next_tools = next_tools(&record_roots, &path_facts);

    let mut report = ProfileReport {
        schema: REPORT_SCHEMA,
        partial: path_report.partial,
        error_count: path_report.error_count,
        errors_truncated: path_report.errors_truncated,
        budget: BudgetReport {
            requested_bytes: options.budget_bytes,
            estimated_bytes: 0,
            truncated: false,
            minimum_bytes_exceeded: false,
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
        next_tools,
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
                &format!("{}[]", display_root_field_path(&field.name)),
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

fn common_values(
    paths: &[PathEntry],
    path_facts: &[PathFact],
    limit: usize,
) -> Vec<CommonValueFact> {
    let fact_order = path_facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (fact.pointer_template.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut entries = paths
        .iter()
        .filter(|entry| entry.display_path != "$")
        .filter(|entry| has_scalar_type(&entry.types))
        .filter(|entry| !entry.samples.is_empty())
        .map(|entry| {
            let signals = path_signals(entry);
            let ranked_bonus = fact_order
                .get(entry.pointer_template.as_str())
                .map(|index| 5_000usize.saturating_sub(*index))
                .unwrap_or(0);
            (path_score(entry, &signals) + ranked_bonus, entry)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.display_path.cmp(&b.1.display_path))
    });

    entries
        .into_iter()
        .take(limit)
        .map(|(_, entry)| CommonValueFact {
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
    report.budget.minimum_bytes_exceeded = false;
    refresh_estimate(report)?;
    if budget_bytes == 0 || report.budget.estimated_bytes <= budget_bytes {
        return Ok(());
    }

    report.budget.truncated = true;
    for step in BUDGET_STEPS {
        apply_budget_step(report, *step);
        refresh_estimate(report)?;
        if report.budget.estimated_bytes <= budget_bytes {
            return Ok(());
        }
    }

    report.budget.minimum_bytes_exceeded = true;
    push_omitted(&mut report.budget.omitted, "budget_minimum_bytes_exceeded");
    refresh_estimate(report)?;
    Ok(())
}

fn refresh_estimate(report: &mut ProfileReport) -> Result<()> {
    let mut estimate = 0;
    for _ in 0..8 {
        report.budget.estimated_bytes = estimate;
        let next = serde_json::to_vec_pretty(report)?.len() + 1;
        if next == estimate {
            return Ok(());
        }
        estimate = next;
    }

    report.budget.estimated_bytes = estimate;
    Ok(())
}

#[derive(Copy, Clone)]
enum BudgetStep {
    Sources(usize),
    Containers(usize),
    RecordRoots(usize),
    PathFacts(usize),
    ShapeFacts(usize),
    TypeVariations(usize),
    CommonValues(usize),
    CommonValueItems(usize),
    Samples(usize),
    SourceFields(usize),
    SourceArrays(usize),
    ClearTypeVariationSamples,
    Errors(usize),
    NextCommands(usize),
    NextTools(usize),
}

const BUDGET_STEPS: &[BudgetStep] = &[
    BudgetStep::PathFacts(24),
    BudgetStep::ShapeFacts(24),
    BudgetStep::TypeVariations(12),
    BudgetStep::CommonValues(16),
    BudgetStep::Samples(16),
    BudgetStep::SourceFields(16),
    BudgetStep::PathFacts(18),
    BudgetStep::ShapeFacts(14),
    BudgetStep::TypeVariations(8),
    BudgetStep::CommonValues(10),
    BudgetStep::Samples(10),
    BudgetStep::CommonValueItems(2),
    BudgetStep::ClearTypeVariationSamples,
    BudgetStep::PathFacts(14),
    BudgetStep::ShapeFacts(10),
    BudgetStep::TypeVariations(5),
    BudgetStep::CommonValues(8),
    BudgetStep::Samples(8),
    BudgetStep::SourceFields(10),
    BudgetStep::SourceArrays(6),
    BudgetStep::Sources(80),
    BudgetStep::Containers(80),
    BudgetStep::RecordRoots(80),
    BudgetStep::PathFacts(10),
    BudgetStep::ShapeFacts(6),
    BudgetStep::TypeVariations(3),
    BudgetStep::CommonValues(4),
    BudgetStep::Samples(4),
    BudgetStep::NextCommands(1),
    BudgetStep::PathFacts(6),
    BudgetStep::ShapeFacts(4),
    BudgetStep::CommonValues(2),
    BudgetStep::Samples(2),
    BudgetStep::TypeVariations(1),
    BudgetStep::Sources(40),
    BudgetStep::Containers(40),
    BudgetStep::RecordRoots(40),
    BudgetStep::Errors(5),
    BudgetStep::NextCommands(0),
    BudgetStep::NextTools(2),
    BudgetStep::CommonValues(0),
    BudgetStep::Samples(0),
    BudgetStep::TypeVariations(0),
    BudgetStep::ShapeFacts(2),
    BudgetStep::PathFacts(3),
    BudgetStep::Sources(12),
    BudgetStep::Containers(12),
    BudgetStep::RecordRoots(12),
    BudgetStep::SourceFields(4),
    BudgetStep::SourceArrays(2),
    BudgetStep::Sources(6),
    BudgetStep::Containers(6),
    BudgetStep::RecordRoots(6),
    BudgetStep::PathFacts(2),
    BudgetStep::ShapeFacts(1),
    BudgetStep::NextTools(1),
    BudgetStep::Errors(2),
    BudgetStep::Sources(3),
    BudgetStep::Containers(3),
    BudgetStep::RecordRoots(3),
    BudgetStep::PathFacts(1),
    BudgetStep::ShapeFacts(0),
    BudgetStep::NextTools(0),
    BudgetStep::Errors(0),
    BudgetStep::Sources(1),
    BudgetStep::Containers(1),
    BudgetStep::RecordRoots(1),
    BudgetStep::PathFacts(0),
    BudgetStep::Sources(0),
    BudgetStep::Containers(0),
    BudgetStep::RecordRoots(0),
];

fn apply_budget_step(report: &mut ProfileReport, step: BudgetStep) {
    match step {
        BudgetStep::Sources(max_len) => truncate_with_note(
            &mut report.sources,
            max_len,
            &mut report.budget.omitted,
            "sources_tail",
        ),
        BudgetStep::Containers(max_len) => truncate_with_note(
            &mut report.containers,
            max_len,
            &mut report.budget.omitted,
            "containers_tail",
        ),
        BudgetStep::RecordRoots(max_len) => truncate_with_note(
            &mut report.record_roots,
            max_len,
            &mut report.budget.omitted,
            "record_roots_tail",
        ),
        BudgetStep::PathFacts(max_len) => truncate_with_note(
            &mut report.path_facts,
            max_len,
            &mut report.budget.omitted,
            "path_facts_tail",
        ),
        BudgetStep::ShapeFacts(max_len) => truncate_with_note(
            &mut report.shape_facts,
            max_len,
            &mut report.budget.omitted,
            "shape_facts_tail",
        ),
        BudgetStep::TypeVariations(max_len) => truncate_with_note(
            &mut report.type_variations,
            max_len,
            &mut report.budget.omitted,
            "type_variations_tail",
        ),
        BudgetStep::CommonValues(max_len) => truncate_with_note(
            &mut report.common_values,
            max_len,
            &mut report.budget.omitted,
            "common_values_tail",
        ),
        BudgetStep::CommonValueItems(max_len) => {
            let mut changed = false;
            for fact in &mut report.common_values {
                if fact.values.len() > max_len {
                    fact.values.truncate(max_len);
                    changed = true;
                }
            }
            if changed {
                push_omitted(&mut report.budget.omitted, "common_value_items_tail");
            }
        }
        BudgetStep::Samples(max_len) => truncate_with_note(
            &mut report.samples,
            max_len,
            &mut report.budget.omitted,
            "samples_tail",
        ),
        BudgetStep::SourceFields(max_len) => {
            let mut changed = false;
            for source in &mut report.sources {
                if source.top_level_fields.len() > max_len {
                    source.top_level_fields.truncate(max_len);
                    changed = true;
                }
            }
            if changed {
                push_omitted(&mut report.budget.omitted, "source_field_tail");
            }
        }
        BudgetStep::SourceArrays(max_len) => {
            let mut changed = false;
            for source in &mut report.sources {
                if source.array_fields.len() > max_len {
                    source.array_fields.truncate(max_len);
                    changed = true;
                }
            }
            if changed {
                push_omitted(&mut report.budget.omitted, "source_array_tail");
            }
        }
        BudgetStep::ClearTypeVariationSamples => {
            let mut changed = false;
            for variation in &mut report.type_variations {
                if !variation.samples.is_empty() {
                    variation.samples.clear();
                    changed = true;
                }
            }
            if changed {
                push_omitted(&mut report.budget.omitted, "type_variation_samples");
            }
        }
        BudgetStep::Errors(max_len) => truncate_with_note(
            &mut report.errors,
            max_len,
            &mut report.budget.omitted,
            "errors_tail",
        ),
        BudgetStep::NextCommands(max_len) => truncate_with_note(
            &mut report.next_commands,
            max_len,
            &mut report.budget.omitted,
            "next_commands_tail",
        ),
        BudgetStep::NextTools(max_len) => truncate_with_note(
            &mut report.next_tools,
            max_len,
            &mut report.budget.omitted,
            "next_tools_tail",
        ),
    }
}

fn truncate_with_note<T>(
    values: &mut Vec<T>,
    max_len: usize,
    omitted: &mut Vec<String>,
    label: &str,
) {
    if values.len() > max_len {
        values.truncate(max_len);
        push_omitted(omitted, label);
    }
}

fn push_omitted(omitted: &mut Vec<String>, label: &str) {
    if !omitted.iter().any(|item| item == label) {
        omitted.push(label.to_string());
    }
}

fn next_tools(record_roots: &[RecordRootFact], path_facts: &[PathFact]) -> Vec<NextToolHint> {
    let root = record_roots.first();
    let candidate = root.and_then(|root| next_path_for_root(root, path_facts));
    let root_label = root.map(|root| root.display_path.as_str()).unwrap_or("$");
    let structural_filter = root
        .map(|root| structural_filter(root, candidate))
        .unwrap_or_else(|| "<filter>".to_string());
    let jaq_command = format!("jaq -c {} <input>", shell_quote(&structural_filter));
    let jq_command = format!("jq -c {} <input>", shell_quote(&structural_filter));
    let jg_pattern = candidate
        .map(|fact| display_path_from_pointer(&fact.pointer_template))
        .or(root.map(|root| root.display_path.clone()))
        .unwrap_or_else(|| "<path-pattern>".to_string());
    let structural_reason = if candidate.is_some() {
        format!("filter from detected record root {root_label} using a narrower observed path")
    } else {
        format!(
            "project detected record root {root_label}; no narrower scalar presence predicate was found"
        )
    };
    let jscan_grep_command = root
        .map(|root| {
            let predicate = candidate
                .and_then(|fact| {
                    relative_pointer_segments(&root.pointer_template, &fact.pointer_template)
                })
                .filter(|segments| !segments.is_empty())
                .map(|segments| display_path_from_segments(&segments))
                .unwrap_or_else(|| "<path>".to_string());

            format!(
                "jscan grep <input> --record-root {} --has {} --json --limit 0",
                shell_quote(&root.display_path),
                shell_quote(&predicate)
            )
        })
        .unwrap_or_else(|| "jscan grep <input> --has '<path>' --json --limit 0".to_string());

    vec![
        NextToolHint {
            tool: "jscan grep".to_string(),
            reason:
                "one-pass structural probe using the detected record root and per-predicate counts"
                    .to_string(),
            caveat: "use jq/jaq when you need transformation rather than reconnaissance"
                .to_string(),
            command: jscan_grep_command,
        },
        NextToolHint {
            tool: "rg".to_string(),
            reason: "fast raw smoke test for rare strings before structural filtering".to_string(),
            caveat: "counts text occurrences, not matching JSON objects".to_string(),
            command: "rg '<term>' <input>".to_string(),
        },
        NextToolHint {
            tool: "jaq".to_string(),
            reason: format!("fast JSON-aware {structural_reason}"),
            caveat: "requires a known filter and JSON-aware semantics".to_string(),
            command: jaq_command,
        },
        NextToolHint {
            tool: "jq".to_string(),
            reason: format!("widely available JSON {structural_reason}"),
            caveat: "can be slower for repeated broad probes".to_string(),
            command: jq_command,
        },
        NextToolHint {
            tool: "jg".to_string(),
            reason: "fast JSON-aware field or path presence checks using observed paths"
                .to_string(),
            caveat: "does not replace jq/jaq for value predicates and aggregation".to_string(),
            command: format!("jg {} <input>", shell_quote(&jg_pattern)),
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
    if mixed_types {
        signals.push("mixed_types".to_string());
    }

    let mut seen_keywords = BTreeSet::new();
    for token in path_tokens(path) {
        if let Some(keyword) = canonical_keyword(&token)
            && seen_keywords.insert(keyword)
        {
            signals.push(format!("keyword:{keyword}"));
        }
    }
    signals
}

fn has_scalar_type(types: &BTreeMap<String, usize>) -> bool {
    ["string", "boolean", "integer", "number", "null"]
        .iter()
        .any(|kind| types.contains_key(*kind))
}

fn next_path_for_root<'a>(
    root: &RecordRootFact,
    path_facts: &'a [PathFact],
) -> Option<&'a PathFact> {
    path_facts
        .iter()
        .enumerate()
        .filter_map(|(index, fact)| {
            let segments =
                relative_pointer_segments(&root.pointer_template, &fact.pointer_template)?;
            if !has_scalar_type(&fact.types)
                || segments.is_empty()
                || !is_selective_path(root, fact)
            {
                return None;
            }

            let array_item_leaf = matches!(segments.last().map(String::as_str), Some("*"));
            Some((array_item_leaf, index, fact))
        })
        .min_by_key(|(array_item_leaf, index, _)| (*array_item_leaf, *index))
        .map(|(_, _, fact)| fact)
}

fn is_selective_path(root: &RecordRootFact, fact: &PathFact) -> bool {
    root.record_count == 0 || fact.count < root.record_count
}

fn structural_filter(root: &RecordRootFact, candidate: Option<&PathFact>) -> String {
    let root_expr = jq_expr_from_pointer(&root.pointer_template);
    let Some(candidate) = candidate else {
        return root_expr;
    };
    let Some(relative_segments) =
        relative_pointer_segments(&root.pointer_template, &candidate.pointer_template)
    else {
        return root_expr;
    };
    let Some(predicate) = presence_predicate(&relative_segments) else {
        return root_expr;
    };

    format!("{root_expr} | select({predicate})")
}

fn presence_predicate(segments: &[String]) -> Option<String> {
    let mut segments = segments.to_vec();
    while matches!(segments.last().map(String::as_str), Some("*")) {
        segments.pop();
    }
    if segments.is_empty() {
        return None;
    }

    Some(format!(
        "{} != null",
        jq_expr_from_segments(&segments, true)
    ))
}

fn jq_expr_from_pointer(pointer: &str) -> String {
    jq_expr_from_segments(&pointer_segments(pointer), false)
}

fn jq_expr_from_segments(segments: &[String], optional_last: bool) -> String {
    if segments.is_empty() {
        return ".".to_string();
    }

    let mut output = String::new();
    for (index, segment) in segments.iter().enumerate() {
        let is_last = index + 1 == segments.len();
        let optional = optional_last && is_last;
        if segment == "*" {
            if output.is_empty() {
                output.push('.');
            }
            output.push_str(if optional { "[]?" } else { "[]" });
        } else if is_jq_identifier(segment) {
            output.push('.');
            output.push_str(segment);
            if optional {
                output.push('?');
            }
        } else {
            if output.is_empty() {
                output.push('.');
            }
            output.push('[');
            output.push_str(&serde_json::to_string(segment).expect("serializing jq key"));
            output.push(']');
            if optional {
                output.push('?');
            }
        }
    }
    output
}

fn relative_pointer_segments(root_pointer: &str, pointer: &str) -> Option<Vec<String>> {
    let root_segments = pointer_segments(root_pointer);
    let path_segments = pointer_segments(pointer);
    if path_segments.len() < root_segments.len()
        || !path_segments
            .iter()
            .zip(root_segments.iter())
            .all(|(path, root)| path == root)
    {
        return None;
    }

    Some(path_segments[root_segments.len()..].to_vec())
}

fn pointer_segments(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return Vec::new();
    }

    pointer
        .trim_start_matches('/')
        .split('/')
        .map(unescape_pointer)
        .collect()
}

fn unescape_pointer(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}

fn is_jq_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn path_tokens(path: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut current = String::new();
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    groups
        .into_iter()
        .flat_map(|group| split_identifier(&group))
        .collect()
}

fn split_identifier(value: &str) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut start = 0;
    for index in 1..chars.len() {
        let previous = chars[index - 1];
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        let boundary = (current.is_ascii_uppercase()
            && (previous.is_ascii_lowercase() || previous.is_ascii_digit()))
            || (current.is_ascii_uppercase()
                && previous.is_ascii_uppercase()
                && next.is_some_and(|ch| ch.is_ascii_lowercase()))
            || (current.is_ascii_digit() != previous.is_ascii_digit());

        if boundary {
            tokens.push(
                chars[start..index]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
            start = index;
        }
    }
    tokens.push(
        chars[start..]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase(),
    );
    tokens
}

fn canonical_keyword(token: &str) -> Option<&'static str> {
    match token {
        "time" => Some("time"),
        "timestamp" => Some("timestamp"),
        "user" | "username" => Some("user"),
        "host" | "hostname" => Some("host"),
        "device" => Some("device"),
        "action" => Some("action"),
        "status" => Some("status"),
        "error" => Some("error"),
        "policy" => Some("policy"),
        "url" => Some("url"),
        "domain" => Some("domain"),
        "ip" => Some("ip"),
        "source" | "src" => Some("source"),
        "destination" | "dest" | "dst" => Some("destination"),
        "connector" => Some("connector"),
        "tunnel" => Some("tunnel"),
        "app" | "application" => Some("app"),
        _ => None,
    }
}

fn under_record_root(pointer: &str, roots: &[&str]) -> bool {
    roots
        .iter()
        .any(|root| relative_pointer_segments(root, pointer).is_some())
}

fn display_root_field_path(key: &str) -> String {
    if is_jq_identifier(key) {
        format!("$.{key}")
    } else {
        format!(
            "$[{}]",
            serde_json::to_string(key).expect("serializing display key")
        )
    }
}

fn display_path_from_pointer(pointer: &str) -> String {
    display_path_from_segments(&pointer_segments(pointer))
}

fn display_path_from_segments(segments: &[String]) -> String {
    if segments.is_empty() {
        return "$".to_string();
    }

    let mut output = "$".to_string();
    for segment in segments {
        if segment == "*" {
            output.push_str("[]");
        } else if is_jq_identifier(segment) {
            output.push('.');
            output.push_str(segment);
        } else {
            output.push('[');
            output.push_str(&serde_json::to_string(&segment).expect("serializing display key"));
            output.push(']');
        }
    }
    output
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
