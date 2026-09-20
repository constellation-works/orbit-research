use std::path::PathBuf;

use crate::output::OutputMode;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    version,
    about = "Capture questions, organize research, and coordinate work through Orbit.",
    after_help = "Start here:\n  orbit-research workspace init ./observatory\n  orbit-research research list --corpus ./observatory\n  orbit-research serve --corpus ./observatory\n\nUse --format json for scripts. Run <COMMAND> --help for details."
)]
pub(crate) struct Cli {
    /// Output format: tables in a terminal, plain text in pipes by default.
    #[arg(
        long = "format",
        global = true,
        value_enum,
        env = "ORBIT_RESEARCH_FORMAT",
        default_value = "auto"
    )]
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
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Loopback port for the dashboard.
        #[arg(long, default_value_t = 4318)]
        port: u16,
    },
    /// Serve research tools over MCP stdio for one explicitly selected corpus.
    Mcp {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Print the packaged native workflow instructions.
    Resource {
        /// Packaged workflow resource version.
        #[arg(long, default_value = "1", value_parser = ["1"])]
        version: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkspaceOperation {
    /// Create a research corpus, or validate one that already exists.
    Init {
        /// Directory to initialize.
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ResearchOperation {
    /// Inspect the configured Orbit backend and compatibility.
    Backend {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Read fresh Orbit task/run evidence for a linked request.
    Status {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
    },
    /// Create an Orbit task from a validated work plan without dispatching it.
    Link {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Path to a JSON work plan returned by a planning command.
        #[arg(long)]
        plan: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
        /// Title of the record or work item.
        #[arg(long)]
        title: String,
        /// Orbit crew assigned to the work.
        #[arg(long)]
        crew: String,
    },
    /// Explicitly approve a linked task for execution.
    Promote {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
    },
    /// Explicitly dispatch an approved linked task.
    Dispatch {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
        /// Git branch to start the Orbit task from.
        #[arg(long)]
        base: String,
    },
    /// Cancel the run currently correlated with a linked task.
    Cancel {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
    },
    /// Validate a persisted Orbit result receipt for a linked task.
    ValidateResult {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
        /// Path to the persisted Orbit result receipt.
        #[arg(long)]
        receipt_path: PathBuf,
    },
    /// List research records and their canonical file paths.
    List {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Show one complete record, including its full title, path, and body.
    Show {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Canonical record ID, such as Q001 or R001.
        #[arg(long)]
        id: String,
    },
    /// Validate the corpus against its owner schema.
    Check {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Reserve an ID and commit a new research record.
    Create {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Record kind: question (Q), hypothesis (H), theory (T), or research (R).
        #[arg(long, value_parser=["Q","H","T","R"])]
        kind: String,
        /// Title of the record or work item.
        #[arg(long)]
        title: String,
        /// Markdown body for the new record.
        #[arg(long, default_value = "")]
        body: String,
        /// Stable request key; reuse it when retrying the same operation.
        #[arg(long)]
        request_key: String,
        /// Tag to attach; repeat for multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Predecessor record ID; repeat to preserve multiple lineage links.
        #[arg(long = "derived-from")]
        derived_from: Vec<String>,
    },
    /// Plan work in a separate code/artifact directory for an existing research item.
    PlanContribution {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Reserved research record ID, such as R001.
        #[arg(long)]
        research_id: String,
        /// Contribution name used for its code and artifact paths.
        #[arg(long)]
        unit: String,
        /// Question or outcome this work should address.
        #[arg(long)]
        objective: String,
    },
    /// Plan an investigation for a reserved research item.
    PlanInvestigation {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Reserved research record ID, such as R001.
        #[arg(long)]
        research_id: String,
        /// Question or outcome this work should address.
        #[arg(long)]
        objective: String,
    },
    /// Plan a follow-up that combines completed contributions.
    PlanSynthesis {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Reserved research record ID, such as R001.
        #[arg(long)]
        research_id: String,
        /// Completed contribution name; repeat for multiple contributions.
        #[arg(long = "unit", required = true)]
        units: Vec<String>,
    },
}
