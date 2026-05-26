use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use jscan::{
    GrepOptions, GrepPredicate, GrepSomeConstraint, InputFormat, InputOptions, MatchMode,
    OutputMode, PathsOptions, ProfileOptions, ShapeOptions, build_profile, collect_grep,
    collect_paths, discover_inputs, infer_shape, parse_path_expr, write_grep_matches,
    write_grep_report, write_path_list, write_paths, write_profile, write_shape,
};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inventory every observed structural path and JSON type.
    Paths(PathsCommand),
    /// Infer object fields, optionality, and array item shapes.
    Shape(ShapeCommand),
    /// Build a bounded reconnaissance profile for agents.
    Profile(ProfileCommand),
    /// Search records with multiple structural predicates in one pass.
    Grep(Box<GrepCommand>),
}

#[derive(Debug, Parser)]
struct PathsCommand {
    #[command(flatten)]
    scan: ScanArgs,

    /// Emit one observed display path per line.
    #[arg(long)]
    plain: bool,
}

#[derive(Debug, Parser)]
struct ShapeCommand {
    #[command(flatten)]
    scan: ScanArgs,
}

#[derive(Debug, Parser)]
struct ProfileCommand {
    #[command(flatten)]
    scan: ScanArgs,

    /// Advisory output budget for JSON profile reports, e.g. 20kb or 1mb.
    #[arg(long, default_value = "20kb")]
    budget: String,
}

#[derive(Debug, Parser)]
struct GrepCommand {
    #[command(flatten)]
    scan: ScanArgs,

    /// Require a path to exist on the record.
    #[arg(long = "has", value_name = "PATH")]
    has: Vec<String>,

    /// Require a path to be absent from the record.
    #[arg(long = "missing", value_name = "PATH")]
    missing: Vec<String>,

    /// Require PATH to equal VALUE. May be repeated.
    #[arg(long = "eq", value_names = ["PATH", "VALUE"], num_args = 2)]
    eq: Vec<String>,

    /// Require PATH to contain VALUE as a string substring or array item.
    #[arg(long = "contains", value_names = ["PATH", "VALUE"], num_args = 2)]
    contains: Vec<String>,

    /// Require ARRAY_PATH to contain an item where ITEM_PATH exists.
    #[arg(long = "some-has", value_names = ["ARRAY_PATH", "ITEM_PATH"], num_args = 2)]
    some_has: Vec<String>,

    /// Require ARRAY_PATH to contain an item where ITEM_PATH equals VALUE.
    #[arg(long = "some-eq", value_names = ["ARRAY_PATH", "ITEM_PATH", "VALUE"], num_args = 3)]
    some_eq: Vec<String>,

    /// Require ARRAY_PATH to contain one item matching all constraints, e.g. action=blocked,user=alice.
    #[arg(long = "some", value_names = ["ARRAY_PATH", "CONSTRAINTS"], num_args = 2)]
    some: Vec<String>,

    /// Treat top-level values at PATH as records before applying predicates.
    #[arg(long, value_name = "PATH")]
    record_root: Option<String>,

    /// Project this path from each matched record and include source/line metadata.
    #[arg(long, value_name = "PATH")]
    show: Option<String>,

    /// Match if any predicate matches. Defaults to requiring all predicates.
    #[arg(long)]
    any: bool,

    /// Emit only the matched record count unless --json is set.
    #[arg(long)]
    count: bool,

    /// Maximum matching records to include in rendered output. Use 0 for none.
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Debug, Args)]
struct ScanArgs {
    /// Files or directories to scan. Use '-' for stdin. Defaults to stdin.
    #[arg(value_name = "INPUT")]
    inputs: Vec<PathBuf>,

    /// Emit stable machine-readable JSON.
    #[arg(long)]
    json: bool,

    /// Output format. Defaults to pretty unless --json is set.
    #[arg(long, value_enum)]
    format: Option<Format>,

    /// Maximum number of parse errors to keep in the report.
    #[arg(long, default_value_t = 20)]
    max_errors: usize,

    /// Maximum number of scalar sample values to retain per path.
    #[arg(long, default_value_t = 0)]
    samples: usize,

    /// Maximum number of characters to keep in a string sample preview.
    #[arg(long, default_value_t = 200)]
    sample_max_chars: usize,

    /// How to parse each input.
    #[arg(long, value_enum, default_value_t = InputFormatArg::Auto)]
    input_format: InputFormatArg,

    /// Include non-JSON-looking files when scanning directories.
    #[arg(long)]
    all_files: bool,

    /// Exit non-zero if any input could not be fully parsed.
    #[arg(long)]
    strict: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Format {
    Pretty,
    Json,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum InputFormatArg {
    Auto,
    Json,
    Jsonl,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Paths(cmd) => {
            ensure_plain_paths_is_compatible(&cmd)?;
            let inputs = discover_inputs(&cmd.scan.inputs, cmd.scan.all_files)?;
            let stdout = io::stdout();
            let mut lock = stdout.lock();
            let report = collect_paths(
                &inputs,
                &input_options(&cmd.scan),
                &PathsOptions {
                    samples_per_path: if cmd.plain { 0 } else { cmd.scan.samples },
                    sample_max_chars: cmd.scan.sample_max_chars,
                },
            )?;
            if cmd.plain {
                write_path_list(&mut lock, &report)?;
                lock.flush()?;
                enforce_strict(cmd.scan.strict, report.partial, report.error_count)?;
            } else {
                write_paths(&mut lock, &report, output_mode(&cmd.scan))?;
                lock.flush()?;
                enforce_strict(cmd.scan.strict, report.partial, report.error_count)?;
            }
        }
        Command::Shape(cmd) => {
            let output = output_mode(&cmd.scan);
            let inputs = discover_inputs(&cmd.scan.inputs, cmd.scan.all_files)?;
            let path_report = collect_paths(
                &inputs,
                &input_options(&cmd.scan),
                &PathsOptions {
                    samples_per_path: cmd.scan.samples,
                    sample_max_chars: cmd.scan.sample_max_chars,
                },
            )?;
            let shape_report = infer_shape(&path_report, &ShapeOptions {})?;

            let stdout = io::stdout();
            let mut lock = stdout.lock();
            write_shape(&mut lock, &shape_report, output)?;
            lock.flush()?;
            enforce_strict(
                cmd.scan.strict,
                shape_report.partial,
                shape_report.error_count,
            )?;
        }
        Command::Profile(cmd) => {
            let output = output_mode(&cmd.scan);
            let inputs = discover_inputs(&cmd.scan.inputs, cmd.scan.all_files)?;
            let samples_per_path = if cmd.scan.samples == 0 {
                2
            } else {
                cmd.scan.samples
            };
            let path_report = collect_paths(
                &inputs,
                &input_options(&cmd.scan),
                &PathsOptions {
                    samples_per_path,
                    sample_max_chars: cmd.scan.sample_max_chars,
                },
            )?;
            let shape_report = infer_shape(&path_report, &ShapeOptions {})?;
            let profile_report = build_profile(
                &path_report,
                &shape_report,
                &ProfileOptions {
                    budget_bytes: parse_byte_size(&cmd.budget)?,
                },
            )?;

            let stdout = io::stdout();
            let mut lock = stdout.lock();
            write_profile(&mut lock, &profile_report, output)?;
            lock.flush()?;
            enforce_strict(
                cmd.scan.strict,
                profile_report.partial,
                profile_report.error_count,
            )?;
        }
        Command::Grep(cmd) => {
            let output = output_mode(&cmd.scan);
            let inputs = discover_inputs(&cmd.scan.inputs, cmd.scan.all_files)?;
            let options = grep_options(&cmd)?;
            let report = collect_grep(&inputs, &input_options(&cmd.scan), &options)?;

            let stdout = io::stdout();
            let mut lock = stdout.lock();
            if cmd.scan.json {
                write_grep_report(&mut lock, &report, output)?;
            } else if cmd.count {
                writeln!(lock, "{}", report.matched_records)?;
            } else {
                write_grep_matches(&mut lock, &report)?;
            }
            lock.flush()?;
            enforce_strict(cmd.scan.strict, report.partial, report.error_count)?;
        }
    }

    Ok(())
}

fn parse_byte_size(value: &str) -> Result<usize> {
    let trimmed = value.trim();
    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (digits, suffix) = trimmed.split_at(split_at);
    if digits.is_empty() {
        anyhow::bail!("budget must start with a number");
    }

    let amount = digits.parse::<usize>()?;
    let multiplier = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        other => anyhow::bail!("unsupported budget suffix: {other}"),
    };

    amount
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("budget is too large"))
}

fn output_mode(args: &ScanArgs) -> OutputMode {
    if args.json {
        return OutputMode::Json;
    }

    match args.format.unwrap_or(Format::Pretty) {
        Format::Pretty => OutputMode::Pretty,
        Format::Json => OutputMode::Json,
    }
}

fn ensure_plain_paths_is_compatible(cmd: &PathsCommand) -> Result<()> {
    if cmd.plain && (cmd.scan.json || cmd.scan.format.is_some()) {
        bail!("--plain cannot be combined with --json or --format");
    }

    Ok(())
}

fn input_options(args: &ScanArgs) -> InputOptions {
    InputOptions {
        format: match args.input_format {
            InputFormatArg::Auto => InputFormat::Auto,
            InputFormatArg::Json => InputFormat::Json,
            InputFormatArg::Jsonl => InputFormat::Jsonl,
        },
        max_errors: args.max_errors,
    }
}

fn grep_options(cmd: &GrepCommand) -> Result<GrepOptions> {
    let mut predicates = Vec::new();

    for path in &cmd.has {
        predicates.push(GrepPredicate::Has(parse_path_expr(path)?));
    }
    for path in &cmd.missing {
        predicates.push(GrepPredicate::Missing(parse_path_expr(path)?));
    }
    for pair in cmd.eq.chunks_exact(2) {
        predicates.push(GrepPredicate::Eq {
            path: parse_path_expr(&pair[0])?,
            value: pair[1].clone(),
        });
    }
    for pair in cmd.contains.chunks_exact(2) {
        predicates.push(GrepPredicate::Contains {
            path: parse_path_expr(&pair[0])?,
            value: pair[1].clone(),
        });
    }
    for pair in cmd.some_has.chunks_exact(2) {
        predicates.push(GrepPredicate::SomeHas {
            path: parse_path_expr(&pair[0])?,
            item_path: parse_path_expr(&pair[1])?,
        });
    }
    for chunk in cmd.some_eq.chunks_exact(3) {
        predicates.push(GrepPredicate::SomeEq {
            path: parse_path_expr(&chunk[0])?,
            item_path: parse_path_expr(&chunk[1])?,
            value: chunk[2].clone(),
        });
    }
    for pair in cmd.some.chunks_exact(2) {
        predicates.push(GrepPredicate::Some {
            path: parse_path_expr(&pair[0])?,
            constraints: parse_some_constraints(&pair[1])?,
        });
    }

    if predicates.is_empty() {
        bail!("grep requires at least one predicate");
    }

    Ok(GrepOptions {
        predicates,
        mode: if cmd.any {
            MatchMode::Any
        } else {
            MatchMode::All
        },
        record_root: cmd
            .record_root
            .as_deref()
            .map(parse_path_expr)
            .transpose()?,
        show_path: cmd.show.as_deref().map(parse_path_expr).transpose()?,
        match_limit: cmd.limit,
    })
}

fn parse_some_constraints(input: &str) -> Result<Vec<GrepSomeConstraint>> {
    let mut constraints = Vec::new();

    for raw_constraint in input.split(',') {
        let constraint = raw_constraint.trim();
        if constraint.is_empty() {
            bail!("--some constraints cannot be empty");
        }

        if let Some((path, value)) = constraint.split_once('=') {
            let path = path.trim();
            if path.is_empty() {
                bail!("--some equality constraints must include a path before '='");
            }
            constraints.push(GrepSomeConstraint::Eq {
                path: parse_path_expr(path)?,
                value: value.trim().to_string(),
            });
        } else {
            constraints.push(GrepSomeConstraint::Has(parse_path_expr(constraint)?));
        }
    }

    if constraints.is_empty() {
        bail!("--some requires at least one constraint");
    }

    Ok(constraints)
}

fn enforce_strict(strict: bool, partial: bool, error_count: usize) -> Result<()> {
    if strict && partial {
        anyhow::bail!("scan completed with {error_count} error(s)");
    }

    Ok(())
}
