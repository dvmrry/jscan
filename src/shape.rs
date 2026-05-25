use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;

use crate::paths::{PathEntry, PathReport, PathSample, PathSegment, ScanError, SourceReport};

const REPORT_SCHEMA: &str = "jscan.shape.v1";

#[derive(Clone, Debug)]
pub struct ShapeOptions {}

#[derive(Clone, Debug, Serialize)]
pub struct ShapeReport {
    pub schema: &'static str,
    pub partial: bool,
    pub error_count: usize,
    pub errors_truncated: bool,
    pub sources: Vec<SourceReport>,
    pub objects: Vec<ObjectShape>,
    pub arrays: Vec<ArrayShape>,
    pub errors: Vec<ScanError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectShape {
    pub display_path: String,
    pub pointer_template: String,
    pub count: usize,
    pub fields: Vec<FieldShape>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldShape {
    pub name: String,
    pub display_path: String,
    pub pointer_template: String,
    pub count: usize,
    pub presence: f64,
    pub required: bool,
    pub types: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<PathSample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArrayShape {
    pub display_path: String,
    pub pointer_template: String,
    pub count: usize,
    pub item_count: usize,
    pub item_types: BTreeMap<String, usize>,
}

pub fn infer_shape(path_report: &PathReport, _options: &ShapeOptions) -> Result<ShapeReport> {
    let by_segments = path_report
        .paths
        .iter()
        .map(|entry| (entry.segments.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let field_children = field_children_by_parent(&path_report.paths);

    let objects = path_report
        .paths
        .iter()
        .filter_map(|entry| {
            let fields = field_children
                .get(&entry.segments)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            object_shape(entry, fields)
        })
        .collect();
    let arrays = path_report
        .paths
        .iter()
        .filter_map(|entry| array_shape(entry, &by_segments))
        .collect();

    Ok(ShapeReport {
        schema: REPORT_SCHEMA,
        partial: path_report.partial,
        error_count: path_report.error_count,
        errors_truncated: path_report.errors_truncated,
        sources: path_report.sources.clone(),
        objects,
        arrays,
        errors: path_report.errors.clone(),
    })
}

fn object_shape(entry: &PathEntry, fields: &[&PathEntry]) -> Option<ObjectShape> {
    let object_count = *entry.types.get("object")?;
    let fields = fields
        .iter()
        .filter_map(|field| {
            let presence = field.count as f64 / object_count as f64;
            Some(FieldShape {
                name: field_name(&entry.segments, &field.segments)?,
                display_path: field.display_path.clone(),
                pointer_template: field.pointer_template.clone(),
                count: field.count,
                presence,
                required: field.count == object_count,
                types: field.types.clone(),
                samples: field.samples.clone(),
            })
        })
        .collect();

    Some(ObjectShape {
        display_path: entry.display_path.clone(),
        pointer_template: entry.pointer_template.clone(),
        count: object_count,
        fields,
    })
}

fn array_shape(
    entry: &PathEntry,
    by_segments: &BTreeMap<Vec<PathSegment>, &PathEntry>,
) -> Option<ArrayShape> {
    let array_count = *entry.types.get("array")?;
    let mut item_segments = entry.segments.clone();
    item_segments.push(PathSegment::ArrayItem);
    let item = by_segments.get(&item_segments)?;

    Some(ArrayShape {
        display_path: entry.display_path.clone(),
        pointer_template: entry.pointer_template.clone(),
        count: array_count,
        item_count: item.count,
        item_types: item.types.clone(),
    })
}

fn field_children_by_parent(paths: &[PathEntry]) -> BTreeMap<Vec<PathSegment>, Vec<&PathEntry>> {
    let mut children = BTreeMap::<Vec<PathSegment>, Vec<&PathEntry>>::new();
    for entry in paths {
        if !matches!(entry.segments.last(), Some(PathSegment::Field { .. })) {
            continue;
        }

        let mut parent = entry.segments.clone();
        parent.pop();
        children.entry(parent).or_default().push(entry);
    }
    children
}

fn field_name(parent: &[PathSegment], field: &[PathSegment]) -> Option<String> {
    match field.get(parent.len()) {
        Some(PathSegment::Field { name }) => Some(name.clone()),
        Some(PathSegment::ArrayItem) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{PathsOptions, collect_paths};
    use crate::{DiscoveredInput, InputOptions};

    #[test]
    fn infers_required_and_optional_fields() {
        let report = collect_paths(
            &[DiscoveredInput::File("tests/fixtures/events.jsonl".into())],
            &InputOptions {
                format: crate::InputFormat::Auto,
                max_errors: 10,
            },
            &PathsOptions {
                samples_per_path: 1,
                sample_max_chars: 200,
            },
        )
        .expect("paths");
        let shape = infer_shape(&report, &ShapeOptions {}).expect("shape");
        let root = shape
            .objects
            .iter()
            .find(|object| object.display_path == "$")
            .expect("root object");

        let status = root
            .fields
            .iter()
            .find(|field| field.name == "status")
            .expect("status field");
        let error = root
            .fields
            .iter()
            .find(|field| field.name == "error")
            .expect("error field");

        assert!(status.required);
        assert_eq!(status.count, 3);
        assert!(!error.required);
        assert_eq!(error.count, 1);
    }
}
