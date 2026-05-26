use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveredInput {
    Stdin,
    File(PathBuf),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputFormat {
    Auto,
    Json,
    Jsonl,
}

#[derive(Clone, Debug)]
pub struct InputOptions {
    pub format: InputFormat,
    pub max_errors: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ContentClassification {
    Empty,
    Json(Value),
    Jsonl,
    InvalidJson { line: usize, message: String },
}

pub fn discover_inputs(inputs: &[PathBuf], all_files: bool) -> Result<Vec<DiscoveredInput>> {
    if inputs.is_empty() {
        return Ok(vec![DiscoveredInput::Stdin]);
    }

    let mut discovered = Vec::new();
    for input in inputs {
        if input == Path::new("-") {
            discovered.push(DiscoveredInput::Stdin);
            continue;
        }

        if input.is_dir() {
            let walker = WalkBuilder::new(input).standard_filters(true).build();
            for entry in walker {
                let entry = entry.with_context(|| format!("walking {}", input.display()))?;
                let file_type = match entry.file_type() {
                    Some(file_type) => file_type,
                    None => continue,
                };
                if file_type.is_file() && (all_files || is_json_candidate(entry.path())) {
                    discovered.push(DiscoveredInput::File(entry.into_path()));
                }
            }
            continue;
        }

        if input.is_file() {
            discovered.push(DiscoveredInput::File(input.clone()));
            continue;
        }

        bail!(
            "input does not exist or is not readable: {}",
            input.display()
        );
    }

    discovered.sort_by_cached_key(input_label);
    Ok(discovered)
}

pub(crate) fn input_label(input: &DiscoveredInput) -> String {
    match input {
        DiscoveredInput::Stdin => "-".to_string(),
        DiscoveredInput::File(path) => path.display().to_string(),
    }
}

pub(crate) fn resolve_input_format(
    input: &DiscoveredInput,
    requested_format: InputFormat,
) -> InputFormat {
    match requested_format {
        InputFormat::Auto if is_jsonl_extension(input) => InputFormat::Jsonl,
        format => format,
    }
}

pub(crate) fn classify_auto_contents(contents: &str) -> ContentClassification {
    if contents.trim().is_empty() {
        return ContentClassification::Empty;
    }

    match serde_json::from_str::<Value>(contents) {
        Ok(value) => ContentClassification::Json(value),
        Err(_) if looks_line_delimited(contents) => ContentClassification::Jsonl,
        Err(error) => ContentClassification::InvalidJson {
            line: error.line(),
            message: error.to_string(),
        },
    }
}

fn is_jsonl_extension(input: &DiscoveredInput) -> bool {
    match input {
        DiscoveredInput::Stdin => false,
        DiscoveredInput::File(path) => has_extension(path, &["jsonl", "ndjson"]),
    }
}

fn looks_line_delimited(contents: &str) -> bool {
    let mut non_empty_lines = 0;
    let mut has_valid_json_line = false;
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        non_empty_lines += 1;
        if matches!(line, "{" | "[" | "}" | "]") {
            return false;
        }
        if !has_valid_json_line && serde_json::from_str::<Value>(line).is_ok() {
            has_valid_json_line = true;
        }
    }

    non_empty_lines > 1 && has_valid_json_line
}

fn is_json_candidate(path: &Path) -> bool {
    has_extension(
        path,
        &["json", "jsonl", "ndjson", "geojson", "har", "sarif"],
    )
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_jsonl_extensions_before_content_sniffing() {
        let input = DiscoveredInput::File("events.ndjson".into());

        assert_eq!(
            resolve_input_format(&input, InputFormat::Auto),
            InputFormat::Jsonl
        );
        assert_eq!(
            resolve_input_format(&input, InputFormat::Json),
            InputFormat::Json
        );
    }

    #[test]
    fn classifies_json_documents() {
        assert_eq!(
            classify_auto_contents(r#"{"items":[1,2]}"#),
            ContentClassification::Json(json!({"items": [1, 2]}))
        );
    }

    #[test]
    fn classifies_opaque_jsonl_by_content() {
        assert_eq!(
            classify_auto_contents("{\"a\":1}\n{\"a\":2}\n"),
            ContentClassification::Jsonl
        );
    }

    #[test]
    fn keeps_malformed_multiline_json_as_json_error() {
        assert!(matches!(
            classify_auto_contents("{\n  \"a\": 1,\n  \"b\": \n}\n"),
            ContentClassification::InvalidJson { .. }
        ));
    }

    #[test]
    fn classifies_empty_inputs() {
        assert_eq!(
            classify_auto_contents(" \n\t"),
            ContentClassification::Empty
        );
    }

    #[test]
    fn classifies_mixed_valid_and_invalid_jsonl_as_jsonl() {
        assert_eq!(
            classify_auto_contents("{\"a\":1}\nnot-json\n"),
            ContentClassification::Jsonl
        );
    }
}
