use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

use clap::{Args, Parser, Subcommand};
use orbit_research_core::legacy_contract::{parse_json, reconcile, validate};
use orbit_research_core::legacy_import::{ADAPTERS, import_source, write_report};
use orbit_research_core::legacy_owner::{Owner, OwnerConfig, reference, write_json_new};
use serde_json::{Value, json};

mod output;

/// A refused request. Carries the fail-closed `problems` an invalid `index` rebuild
/// reports alongside its message, per `specs/cli-compat.md`.
struct Invalid {
    message: String,
    problems: Option<Vec<Value>>,
}

impl From<String> for Invalid {
    fn from(message: String) -> Self {
        Invalid {
            message,
            problems: None,
        }
    }
}

impl From<orbit_research_core::legacy_index::IndexError> for Invalid {
    fn from(error: orbit_research_core::legacy_index::IndexError) -> Self {
        let problems = error.problems().map(|problems| {
            problems
                .iter()
                .map(orbit_research_core::legacy_index::Problem::to_value)
                .collect()
        });
        Invalid {
            message: error.to_string(),
            problems,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Cli {
    /// Output mode for new commands. Legacy commands retain JSON by default.
    #[arg(long = "format", global = true, value_enum, default_value = "json")]
    format: output::OutputMode,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a new corpus or validate an existing corpus without modifying it.
    Workspace {
        #[command(subcommand)]
        operation: WorkspaceOperation,
    },
    /// Canonical Markdown research operations, shared with the MCP server.
    Research {
        #[command(subcommand)]
        operation: ResearchOperation,
    },
    /// Serve the local research dashboard.
    Serve {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long, default_value_t = 4318)]
        port: u16,
    },
    /// Serve research tools over MCP stdio for one explicitly selected corpus.
    Mcp {
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Print the packaged native workflow instructions.
    Resource {
        #[arg(long, default_value = "1", value_parser = ["1"])]
        version: String,
    },
    /// Read one explicitly routed assigned Orbit task through the registered CLI.
    TaskContext {
        #[arg(long = "orbit-root")]
        orbit_root: PathBuf,
        #[arg(long)]
        host: String,
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        run: String,
        #[arg(long = "orbit-executable", default_value = "orbit")]
        orbit_executable: PathBuf,
    },
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
    /// Atomic disposable rebuild of the cross-owner SQLite projection.
    Index {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        database: PathBuf,
    },
    /// The exact indexed snapshot, its assessments and their transitive dependencies.
    IndexTrace {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        key: String,
    },
    /// Portable static export of the published projection; the destination must be new.
    BrowseExport {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inventory sources and emit read-only migration candidates. Always a
    /// dry run: `--dry-run` is accepted for documentation and never changes
    /// behavior, since there is no non-dry-run mode.
    Import {
        adapter: String,
        #[arg(long = "source-root")]
        source_root: PathBuf,
        #[arg(long, help = "stable owner namespace, independent of filesystem path")]
        repository: String,
        #[arg(
            long = "expect-revision",
            help = "exact Git HEAD required before reading"
        )]
        expect_revision: Option<String>,
        #[arg(
            long = "select",
            help = "relative input file; repeat to override default discovery"
        )]
        select: Vec<String>,
        #[arg(long = "dry-run", help = "default and only supported mode")]
        dry_run: bool,
        #[arg(long, help = "new output file outside source root; default stdout")]
        output: Option<PathBuf>,
    },
    Program(Authoring),
    Claim(Authoring),
    Artifact(Authoring),
    Preregister(Authoring),
    BeginRun(Authoring),
    RecordRun(Authoring),
    Assess(Authoring),
    Retire(Authoring),
    Heads {
        #[command(flatten)]
        owner: OwnerArgs,
        /// Full canonical URN; identities are never inferred from a short name.
        #[arg(long)]
        id: String,
    },
    Ref {
        #[command(flatten)]
        owner: OwnerArgs,
        #[arg(long)]
        id: String,
        #[arg(long)]
        revision: String,
        #[arg(long = "source-revision")]
        source_revision: String,
    },
    Trace {
        #[command(flatten)]
        owner: OwnerArgs,
        #[arg(long)]
        id: String,
        #[arg(long)]
        revision: String,
    },
    Export {
        #[command(flatten)]
        owner: OwnerArgs,
        #[arg(long = "source-revision")]
        source_revision: String,
        /// New destination outside the canonical records directory.
        #[arg(long)]
        output: PathBuf,
    },
}

/// Explicit owner routing shared by every owner subcommand.
#[derive(Args, Debug)]
struct OwnerArgs {
    #[arg(long = "owner-root")]
    owner_root: PathBuf,
    #[arg(long)]
    repository: String,
    #[arg(long, default_value = "research/records")]
    records: String,
    /// Routed source checkout as `REPOSITORY=ROOT`; repeatable.
    #[arg(long = "source", value_name = "REPOSITORY=ROOT")]
    sources: Vec<String>,
}

#[derive(Args, Debug)]
struct Authoring {
    #[command(flatten)]
    owner: OwnerArgs,
    /// Request document for this operation.
    #[arg(long)]
    request: PathBuf,
}

impl OwnerArgs {
    fn open(&self) -> Result<Owner, String> {
        let mut sources = std::collections::BTreeMap::new();
        for value in &self.sources {
            let (name, root) = value
                .split_once('=')
                .filter(|(name, root)| !name.is_empty() && !root.is_empty())
                .ok_or_else(|| {
                    "source routing requires unique REPOSITORY=ROOT mappings".to_owned()
                })?;
            if sources
                .insert(name.to_owned(), PathBuf::from(root))
                .is_some()
            {
                return Err("source routing requires unique REPOSITORY=ROOT mappings".to_owned());
            }
        }
        Owner::open(
            &self.owner_root,
            &self.repository,
            OwnerConfig {
                records: self.records.clone(),
                sources,
                ..OwnerConfig::default()
            },
        )
        .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Subcommand)]
enum WorkspaceOperation {
    Init { path: PathBuf },
}
#[derive(Debug, Subcommand)]
enum ResearchOperation {
    List {
        #[arg(long)]
        corpus: PathBuf,
    },
    Check {
        #[arg(long)]
        corpus: PathBuf,
    },
    Create {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long, value_parser=["Q","H","T","R"])]
        kind: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long)]
        request_key: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "derived-from")]
        derived_from: Vec<String>,
    },
    PlanContribution {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        research_id: String,
        #[arg(long)]
        unit: String,
        #[arg(long)]
        objective: String,
    },
    PlanInvestigation {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        research_id: String,
        #[arg(long)]
        objective: String,
    },
    PlanSynthesis {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        research_id: String,
        #[arg(long = "unit", required = true)]
        units: Vec<String>,
    },
}

fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => run(cli),
        Err(error) if error.exit_code() == 0 => {
            let _ = error.print();
            ExitCode::SUCCESS
        }
        Err(error) => invalid(error.to_string().into(), 2),
    }
}

fn run(cli: Cli) -> ExitCode {
    // MCP owns stdout for the whole session; do not emit a trailing CLI envelope.
    if let Command::Mcp { corpus } = &cli.command {
        return match orbit_research_mcp::serve_mcp(corpus, io::stdin().lock(), io::stdout().lock())
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    // Legacy commands retain their established invalid-input exit contract.
    let error_code = if matches!(
        &cli.command,
        Command::Research { .. } | Command::Workspace { .. } | Command::Serve { .. }
    ) {
        1
    } else {
        2
    };
    let mode = cli.format;
    match execute(cli) {
        Ok((output, code)) => {
            let interactive = io::stdout().is_terminal();
            match output::render_with_terminal(&mut io::stdout().lock(), &output, mode, interactive)
            {
                Ok(()) => ExitCode::from(code),
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
        }
        Err(error) => invalid(error, error_code),
    }
}

fn execute(cli: Cli) -> Result<(Value, u8), Invalid> {
    match cli.command {
        Command::Mcp { .. } => Err("MCP must run as a stdio session".to_string().into()),
        Command::Serve { corpus, port } => {
            orbit_research_web::serve(
                orbit_research_core::Research::open(&corpus).map_err(|e| e.to_string())?,
                port,
            )
            .map_err(|e| e.to_string())?;
            Ok((json!({"stopped":true}), 0))
        }
        Command::Workspace {
            operation: WorkspaceOperation::Init { path },
        } => Ok((
            orbit_research_core::init_workspace(&path).map_err(|e| e.to_string())?,
            0,
        )),
        Command::Research { operation } => {
            let (corpus, name, input) = match operation {
                ResearchOperation::List { corpus } => (corpus, "research.list", json!({})),
                ResearchOperation::Check { corpus } => (corpus, "research.check", json!({})),
                ResearchOperation::Create {
                    corpus,
                    kind,
                    title,
                    body,
                    request_key,
                    tags,
                    derived_from,
                } => (
                    corpus,
                    "research.create",
                    json!({"kind":kind,"title":title,"body":body,"request_key":request_key,"tags":tags,"derived_from":derived_from}),
                ),
                ResearchOperation::PlanContribution {
                    corpus,
                    research_id,
                    unit,
                    objective,
                } => (
                    corpus,
                    "research.plan_contribution",
                    json!({"research_id":research_id,"unit":unit,"objective":objective}),
                ),
                ResearchOperation::PlanInvestigation {
                    corpus,
                    research_id,
                    objective,
                } => (
                    corpus,
                    "research.plan_investigation",
                    json!({"research_id":research_id,"objective":objective}),
                ),
                ResearchOperation::PlanSynthesis {
                    corpus,
                    research_id,
                    units,
                } => (
                    corpus,
                    "research.plan_synthesis",
                    json!({"research_id":research_id,"units":units}),
                ),
            };
            Ok((
                orbit_research_core::api::call(&corpus, name, input).map_err(|e| e.to_string())?,
                0,
            ))
        }
        Command::Resource { version: _ } => Ok((
            json!({
                "version": 1,
                "skill": include_str!("../resources/v1/SKILL.md"),
            }),
            0,
        )),
        Command::TaskContext {
            orbit_root,
            host,
            workspace,
            task,
            run,
            orbit_executable,
        } => task_context(
            &orbit_root,
            &host,
            &workspace,
            &task,
            &run,
            &orbit_executable,
        ),
        Command::Validate { input, targets } => {
            let document = read_json(&input)?;
            let targets = read_targets(&targets)?;
            let errors = validate(&document, &targets);
            let code = if errors.is_empty() { 0 } else { 1 };
            Ok((json!({"valid": errors.is_empty(), "errors": errors}), code))
        }
        Command::Program(args) => Ok(author("program", &args)?),
        Command::Claim(args) => Ok(author("claim", &args)?),
        Command::Artifact(args) => Ok(author("artifact", &args)?),
        Command::Preregister(args) => Ok(author("preregister", &args)?),
        Command::BeginRun(args) => Ok(author("begin-run", &args)?),
        Command::RecordRun(args) => Ok(author("record-run", &args)?),
        Command::Assess(args) => Ok(author("assess", &args)?),
        Command::Retire(args) => Ok(author("retire", &args)?),
        Command::Heads { owner, id } => {
            let owner = owner.open()?;
            let heads = owner.heads(&id).map_err(|error| error.to_string())?;
            Ok((json!({"heads": heads}), 0))
        }
        Command::Ref {
            owner,
            id,
            revision,
            source_revision,
        } => {
            let owner = owner.open()?;
            let pinned = owner
                .pin(&id, &revision, &source_revision)
                .map_err(|error| error.to_string())?;
            Ok((reference(&pinned, "resolved"), 0))
        }
        Command::Trace {
            owner,
            id,
            revision,
        } => {
            let owner = owner.open()?;
            let trace = owner
                .trace(&id, &revision)
                .map_err(|error| error.to_string())?;
            Ok((trace, 0))
        }
        Command::Export {
            owner,
            source_revision,
            output,
        } => {
            let owner = owner.open()?;
            let destination = resolved_destination(&output)?;
            if destination.starts_with(
                owner
                    .directory()
                    .canonicalize()
                    .unwrap_or_else(|_| owner.directory().to_path_buf()),
            ) {
                return Err("export cannot write inside canonical records"
                    .to_owned()
                    .into());
            }
            let bundle = owner
                .export(&source_revision)
                .map_err(|error| error.to_string())?;
            write_json_new(&bundle, &output).map_err(|error| error.to_string())?;
            Ok((
                json!({
                    "output": output.to_string_lossy(),
                    "records": bundle.get("records").and_then(Value::as_array).map_or(0, Vec::len),
                    "manifests": bundle.get("manifests").and_then(Value::as_array).map_or(0, Vec::len),
                }),
                0,
            ))
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
                return Err(errors.join("; ").into());
            }
            write_new(&output, &result)?;
            Ok((json!({"output": output.to_string_lossy()}), 0))
        }
        Command::Index { config, database } => {
            let outcome = orbit_research_core::legacy_index::rebuild(&config, &database)?;
            Ok((
                json!({
                    "database": outcome.database.to_string_lossy(),
                    "records": outcome.records,
                    "content_digest": outcome.content_digest,
                    "pending": outcome.pending,
                }),
                0,
            ))
        }
        Command::IndexTrace { database, key } => {
            let trace = orbit_research_core::legacy_index::trace(&database, &key)?;
            Ok((trace, 0))
        }
        Command::BrowseExport {
            config,
            database,
            output,
        } => {
            let outcome =
                orbit_research_core::legacy_index::export_browser(&database, &output, &config)?;
            Ok((
                json!({
                    "output": outcome.output.to_string_lossy(),
                    "records": outcome.records,
                    "content_digest": outcome.content_digest,
                }),
                0,
            ))
        }
        Command::Import {
            adapter,
            source_root,
            repository,
            expect_revision,
            select,
            dry_run: _,
            output,
        } => {
            if !ADAPTERS.contains(&adapter.as_str()) {
                return Err(format!("adapter must be one of: {}", ADAPTERS.join(", ")).into());
            }
            let selected = if select.is_empty() {
                None
            } else {
                Some(select.as_slice())
            };
            let report = import_source(
                &source_root,
                &adapter,
                &repository,
                selected,
                expect_revision.as_deref(),
            )
            .map_err(|error| error.to_string())?;
            let errors = validate(&report, &[]);
            if !errors.is_empty() {
                return Err(format!(
                    "candidate validation failed: {}",
                    errors
                        .iter()
                        .take(20)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                )
                .into());
            }
            if let Some(output) = output {
                write_report(&report, &output, &[source_root])
                    .map_err(|error| error.to_string())?;
                Ok((
                    json!({
                        "output": output.to_string_lossy(),
                        "counts": report["counts"],
                        "source_unchanged": true,
                    }),
                    0,
                ))
            } else {
                Ok((report, 0))
            }
        }
    }
}

/// Use Orbit only through its registered CLI protocol. Keeping this adapter in the binary
/// avoids coupling any library crate to Orbit's task engine or store.
fn task_context(
    orbit_root: &Path,
    host: &str,
    workspace: &str,
    task: &str,
    run: &str,
    orbit_executable: &Path,
) -> Result<(Value, u8), Invalid> {
    if !orbit_root.is_absolute() {
        return Err("explicit absolute Orbit authority root required"
            .to_owned()
            .into());
    }
    if [host, workspace, task, run]
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err("explicit host/workspace/task/run required"
            .to_owned()
            .into());
    }
    let request = json!({"id": task, "workspace": workspace, "model": "codex"});
    let output = ProcessCommand::new(orbit_executable)
        .args(["tool", "run", "orbit.task.show", "--root"])
        .arg(orbit_root)
        .arg("--input")
        .arg(request.to_string())
        .output()
        .map_err(|error| format!("{}: {error}", orbit_executable.display()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("Orbit task lookup exited with {}", output.status)
        } else {
            detail
        }
        .into());
    }
    let mut value =
        parse_json(&output.stdout).map_err(|error| format!("invalid Orbit response: {error}"))?;
    if let Some(result) = value.get("result").filter(|item| item.is_object()) {
        value = result.clone();
    }
    if value.get("id").and_then(Value::as_str) != Some(task) {
        return Err("Orbit response does not identify assigned task"
            .to_owned()
            .into());
    }
    if value
        .get("workspace")
        .and_then(|owner| owner.get("id"))
        .and_then(Value::as_str)
        != Some(workspace)
    {
        return Err("Orbit response workspace differs from explicit authority"
            .to_owned()
            .into());
    }
    let terminal = value.get("terminal").and_then(Value::as_bool) == Some(true)
        || matches!(
            value.get("status").and_then(Value::as_str),
            Some("done" | "rejected")
        );
    if terminal {
        return Err("assigned task is terminal".to_owned().into());
    }
    Ok((
        json!({
            "task": value,
            "orbit_link": {
                "host": host,
                "workspace": workspace,
                "task": task,
                "run": run,
            },
        }),
        0,
    ))
}

/// Append one immutable record through the owner crate and return it verbatim.
fn author(operation: &str, args: &Authoring) -> Result<(Value, u8), String> {
    let owner = args.owner.open()?;
    let request = read_json(&args.request)?;
    let record = owner
        .apply(operation, &request)
        .map_err(|error| error.to_string())?;
    Ok((record, 0))
}

/// Resolve a not-yet-existing destination through its parent directory.
fn resolved_destination(output: &Path) -> Result<PathBuf, String> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = match parent {
        Some(parent) => parent
            .canonicalize()
            .map_err(|error| format!("{}: {error}", parent.display()))?,
        None => std::env::current_dir().map_err(|error| error.to_string())?,
    };
    Ok(match output.file_name() {
        Some(name) => directory.join(name),
        None => directory,
    })
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

fn invalid(error: Invalid, code: u8) -> ExitCode {
    let mut payload = json!({"error":{"code":"invalid-input","message":error.message}});
    if let Some(problems) = error.problems {
        payload["error"]["problems"] = json!(problems);
    }
    emit(io::stderr().lock(), &payload);
    ExitCode::from(code)
}

fn emit(mut stream: impl io::Write, value: &Value) {
    let _ = serde_json::to_writer(&mut stream, value);
    let _ = stream.write_all(b"\n");
}
