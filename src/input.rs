use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use serde::Serialize;

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

pub(crate) fn is_jsonl_candidate(input: &DiscoveredInput) -> bool {
    match input {
        DiscoveredInput::Stdin => false,
        DiscoveredInput::File(path) => has_extension(path, &["jsonl", "ndjson"]),
    }
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
