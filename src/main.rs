use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use jscan::{
    InputFormat, InputOptions, OutputMode, PathsOptions, ProfileOptions, ShapeOptions,
    build_profile, collect_paths, discover_inputs, infer_shape, write_path_list, write_paths,
    write_profile, write_shape,
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
            let report = collect_paths(
                &inputs,
                &input_options(&cmd.scan),
                &PathsOptions {
                    samples_per_path: cmd.scan.samples,
                    sample_max_chars: cmd.scan.sample_max_chars,
                },
            )?;

            let stdout = io::stdout();
            let mut lock = stdout.lock();
            if cmd.plain {
                write_path_list(&mut lock, &report)?;
            } else {
                write_paths(&mut lock, &report, output_mode(&cmd.scan))?;
            }
            lock.flush()?;
            enforce_strict(cmd.scan.strict, report.partial, report.error_count)?;
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

fn enforce_strict(strict: bool, partial: bool, error_count: usize) -> Result<()> {
    if strict && partial {
        anyhow::bail!("scan completed with {error_count} error(s)");
    }

    Ok(())
}
