use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use orbit_research_contract::{parse_json, reconcile, validate};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Validate {
        input: PathBuf,
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
    },
    Reconcile {
        input: PathBuf,
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => run(cli),
        Err(error) => invalid(error.to_string()),
    }
}

fn run(cli: Cli) -> ExitCode {
    match execute(cli) {
        Ok((output, code)) => {
            emit(io::stdout().lock(), &output);
            ExitCode::from(code)
        }
        Err(message) => invalid(message),
    }
}

fn execute(cli: Cli) -> Result<(Value, u8), String> {
    match cli.command {
        Command::Validate { input, targets } => {
            let document = read_json(&input)?;
            let targets = read_targets(&targets)?;
            let errors = validate(&document, &targets);
            let code = if errors.is_empty() { 0 } else { 1 };
            Ok((json!({"valid": errors.is_empty(), "errors": errors}), code))
        }
        Command::Reconcile {
            input,
            targets,
            output,
        } => {
            let manifest = read_json(&input)?;
            let targets = read_targets(&targets)?;
            let result = reconcile(&manifest, &targets).map_err(|error| error.to_string())?;
            let errors = validate(&result, &targets);
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            write_new(&output, &result)?;
            Ok((json!({"output": output.to_string_lossy()}), 0))
        }
    }
}

fn read_targets(paths: &[PathBuf]) -> Result<Vec<Value>, String> {
    paths.iter().map(|path| read_json(path)).collect()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_json(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_new(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

fn invalid(message: String) -> ExitCode {
    emit(
        io::stderr().lock(),
        &json!({"error":{"code":"invalid-input","message":message}}),
    );
    ExitCode::from(2)
}

fn emit(mut stream: impl io::Write, value: &Value) {
    let _ = serde_json::to_writer(&mut stream, value);
    let _ = stream.write_all(b"\n");
}
