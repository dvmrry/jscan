use std::io::Write;

use anyhow::Result;

use crate::paths::PathReport;
use crate::profile::ProfileReport;
use crate::shape::ShapeReport;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Pretty,
    Json,
}

pub fn write_paths<W: Write>(writer: &mut W, report: &PathReport, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Json => {
            serde_json::to_writer_pretty(&mut *writer, report)?;
            writeln!(writer)?;
        }
        OutputMode::Pretty => write_pretty_paths(writer, report)?,
    }

    Ok(())
}

pub fn write_path_list<W: Write>(writer: &mut W, report: &PathReport) -> Result<()> {
    for entry in &report.paths {
        writeln!(writer, "{}", entry.display_path)?;
    }

    Ok(())
}

pub fn write_shape<W: Write>(writer: &mut W, report: &ShapeReport, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Json => {
            serde_json::to_writer_pretty(&mut *writer, report)?;
            writeln!(writer)?;
        }
        OutputMode::Pretty => write_pretty_shape(writer, report)?,
    }

    Ok(())
}

pub fn write_profile<W: Write>(
    writer: &mut W,
    report: &ProfileReport,
    mode: OutputMode,
) -> Result<()> {
    match mode {
        OutputMode::Json => {
            serde_json::to_writer_pretty(&mut *writer, report)?;
            writeln!(writer)?;
        }
        OutputMode::Pretty => write_pretty_profile(writer, report)?,
    }

    Ok(())
}

fn write_pretty_paths<W: Write>(writer: &mut W, report: &PathReport) -> Result<()> {
    writeln!(writer, "PATH\tCOUNT\tTYPES")?;
    for entry in &report.paths {
        let types = entry
            .types
            .iter()
            .map(|(kind, count)| format!("{kind}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(writer, "{}\t{}\t{}", entry.display_path, entry.count, types)?;
    }

    if !report.errors.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "ERRORS")?;
        for error in &report.errors {
            match error.line {
                Some(line) => writeln!(writer, "{}:{}\t{}", error.source, line, error.message)?,
                None => writeln!(writer, "{}\t{}", error.source, error.message)?,
            }
        }
    }

    Ok(())
}

fn write_pretty_profile<W: Write>(writer: &mut W, report: &ProfileReport) -> Result<()> {
    writeln!(
        writer,
        "PROFILE\tpartial:{}\terrors:{}\testimated_bytes:{}",
        report.partial, report.error_count, report.budget.estimated_bytes
    )?;

    if !report.containers.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "SOURCE\tCONTAINER\tCONFIDENCE\tREASON")?;
        for container in &report.containers {
            writeln!(
                writer,
                "{}\t{}\t{:.2}\t{}",
                container.source, container.kind, container.confidence, container.reason
            )?;
        }
    }

    if !report.record_roots.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "SOURCE\tRECORD_ROOT\tCONFIDENCE\tCOUNT\tREASON")?;
        for root in &report.record_roots {
            writeln!(
                writer,
                "{}\t{}\t{:.2}\t{}\t{}",
                root.source, root.display_path, root.confidence, root.record_count, root.reason
            )?;
        }
    }

    if !report.path_facts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "PATH\tCOUNT\tTYPES\tSIGNALS")?;
        for fact in &report.path_facts {
            let types = fact
                .types
                .iter()
                .map(|(kind, count)| format!("{kind}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                writer,
                "{}\t{}\t{}\t{}",
                fact.display_path,
                fact.count,
                types,
                fact.signals.join(",")
            )?;
        }
    }

    Ok(())
}

fn write_pretty_shape<W: Write>(writer: &mut W, report: &ShapeReport) -> Result<()> {
    writeln!(writer, "OBJECT\tCOUNT\tFIELD\tPRESENCE\tTYPES")?;
    for object in &report.objects {
        if object.fields.is_empty() {
            writeln!(writer, "{}\t{}\t-\t-\t-", object.display_path, object.count)?;
            continue;
        }

        for field in &object.fields {
            let types = field
                .types
                .iter()
                .map(|(kind, count)| format!("{kind}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            let presence = if field.required {
                "required".to_string()
            } else {
                format!("{:.1}%", field.presence * 100.0)
            };
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}",
                object.display_path, object.count, field.name, presence, types
            )?;
        }
    }

    if !report.arrays.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "ARRAY\tCOUNT\tITEMS\tITEM_TYPES")?;
        for array in &report.arrays {
            let item_types = array
                .item_types
                .iter()
                .map(|(kind, count)| format!("{kind}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                writer,
                "{}\t{}\t{}\t{}",
                array.display_path, array.count, array.item_count, item_types
            )?;
        }
    }

    if !report.errors.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "ERRORS")?;
        for error in &report.errors {
            match error.line {
                Some(line) => writeln!(writer, "{}:{}\t{}", error.source, line, error.message)?,
                None => writeln!(writer, "{}\t{}", error.source, error.message)?,
            }
        }
    }

    Ok(())
}
