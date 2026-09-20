use std::path::PathBuf;

use crate::output::OutputMode;
use clap::{Args, Parser, Subcommand};
use orbit_research_core::legacy_owner::{Owner, OwnerConfig};

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub(crate) struct Cli {
    /// Output mode for new commands. Legacy commands retain JSON by default.
    #[arg(long = "format", global = true, value_enum, default_value = "json")]
    pub(crate) format: OutputMode,
    /// Optional process-scoped Orbit backend configuration JSON.
    #[arg(long = "backend-config", global = true)]
    pub(crate) backend_config: Option<PathBuf>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
pub(crate) struct OwnerArgs {
    #[arg(long = "owner-root")]
    pub(crate) owner_root: PathBuf,
    #[arg(long)]
    pub(crate) repository: String,
    #[arg(long, default_value = "research/records")]
    pub(crate) records: String,
    /// Routed source checkout as `REPOSITORY=ROOT`; repeatable.
    #[arg(long = "source", value_name = "REPOSITORY=ROOT")]
    pub(crate) sources: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct Authoring {
    #[command(flatten)]
    pub(crate) owner: OwnerArgs,
    /// Request document for this operation.
    #[arg(long)]
    pub(crate) request: PathBuf,
}

impl OwnerArgs {
    pub(crate) fn open(&self) -> Result<Owner, String> {
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
pub(crate) enum WorkspaceOperation {
    Init { path: PathBuf },
}
#[derive(Debug, Subcommand)]
pub(crate) enum ResearchOperation {
    /// Inspect the configured Orbit backend and compatibility.
    Backend {
        #[arg(long)]
        corpus: PathBuf,
    },
    /// List request-key to Orbit task correlations.
    Status {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        request_key: String,
    },
    /// Create an Orbit task from a validated work plan without dispatching it.
    Link {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        request_key: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        crew: String,
    },
    /// Explicitly approve a linked task for execution.
    Promote {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        request_key: String,
    },
    /// Explicitly dispatch an approved linked task.
    Dispatch {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        request_key: String,
        #[arg(long)]
        base: String,
    },
    /// Cancel the run currently correlated with a linked task.
    Cancel {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        request_key: String,
    },
    /// Validate a persisted Orbit result receipt for a linked task.
    ValidateResult {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        request_key: String,
        #[arg(long)]
        receipt_path: PathBuf,
    },
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
