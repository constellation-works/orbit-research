use std::path::PathBuf;

use crate::output::OutputMode;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    version,
    about = "Capture questions, organize research, and coordinate work through Orbit.",
    after_help = "Start here:\n  orbit-research workspace init ./observatory\n  orbit-research research list --corpus ./observatory\n\nUse --format json for scripts. Run <COMMAND> --help for details."
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
    /// Shorthand for --format json.
    #[arg(long, global = true)]
    pub(crate) json: bool,
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
    ///
    /// Writes run in one of two modes, detected from the corpus checkout and
    /// reported as `mode` in every write result. Primary mode (the primary
    /// checkout) allocates IDs and commits each write under a lock. Worktree
    /// mode (a linked run worktree) writes only the run's reserved research
    /// record, never allocates IDs and never commits: the run's commit step does.
    /// Pass --mode on a write to refuse when detection disagrees.
    Research {
        #[command(subcommand)]
        operation: Box<ResearchOperation>,
    },
    /// Serve research tools over MCP stdio for one explicitly selected corpus.
    Mcp {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Serve one Orbit plugin tool call over the sandboxed `exec` backend protocol
    /// (one stdin JSON request, one stdout JSON reply). Not for interactive use.
    OrbitTool,
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
    /// Reserve an ID and commit a new research record (primary mode only).
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
        /// Initial status; `--kind R --status planned` is the reservation.
        #[arg(long, value_parser=["open","planned","active"])]
        status: Option<String>,
        #[command(flatten)]
        mode: ModeArg,
    },
    /// Capture a new question from text and tags alone (primary mode only).
    Capture {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Question text; its first line becomes the title.
        #[arg(long)]
        text: String,
        /// Tag to attach; repeat for multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Stable request key; defaults to one derived from the text and tags.
        #[arg(long)]
        request_key: Option<String>,
        #[command(flatten)]
        mode: ModeArg,
    },
    /// Revise a record under its expected blob. Primary mode commits Q/H/T
    /// edits; worktree mode writes only the run's reserved R, uncommitted.
    Revise {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Record ID, such as H001 or R001.
        #[arg(long)]
        id: String,
        /// Git blob the record had when read (`show` reports it as git_blob).
        #[arg(long)]
        expected_blob: String,
        /// New title; the record path stays frozen.
        #[arg(long)]
        title: Option<String>,
        /// New Markdown body. A hypothesis title or body change bumps its revision.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read the new Markdown body from a file.
        #[arg(long)]
        body_file: Option<PathBuf>,
        /// Replacement tag; repeat for multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// New status, as the owner schema allows for the record's kind.
        #[arg(long)]
        status: Option<String>,
        /// Research only: tested hypothesis ID; repeat for several.
        #[arg(long = "test")]
        tests: Vec<String>,
        /// Research only: Orbit task that produced the result.
        #[arg(long)]
        orbit_task: Option<String>,
        /// Research only: Orbit run that produced the result.
        #[arg(long)]
        orbit_run: Option<String>,
        /// Research only: JSON file replacing data/manifest.json.
        #[arg(long)]
        manifest_file: Option<PathBuf>,
        #[command(flatten)]
        mode: ModeArg,
    },
    /// Append a verdict to a hypothesis's assessments (primary mode only).
    /// Needs an existing revision and an accepted research record.
    Assess {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Hypothesis ID, such as H001.
        #[arg(long)]
        id: String,
        /// Git blob the hypothesis had when read.
        #[arg(long)]
        expected_blob: String,
        /// Accepted research record the verdict rests on, such as R001.
        #[arg(long)]
        research: String,
        /// Hypothesis revision the verdict is about.
        #[arg(long)]
        revision: u64,
        /// Your verdict. Execution success is never support.
        #[arg(long, value_parser=["supports","refutes","inconclusive"])]
        verdict: String,
        /// Strength of the evidence.
        #[arg(long, value_parser=["anecdote","suggestive","strong"])]
        strength: String,
        /// Short justification, such as failed controls.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        mode: ModeArg,
    },
    /// Revise a question's title, body and tags under its expected blob (primary mode only).
    ReviseQuestion {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Question ID, such as Q001.
        #[arg(long)]
        id: String,
        /// Git blob the question had when read.
        #[arg(long)]
        expected_blob: String,
        /// New title; the record path stays frozen.
        #[arg(long)]
        title: String,
        /// New Markdown body.
        #[arg(long, default_value = "")]
        body: String,
        /// Replacement tag; repeat for multiple tags.
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[command(flatten)]
        mode: ModeArg,
    },
    /// List local request correlations pointing to Orbit tasks (cached, not live status).
    WorkLinks {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
    },
    /// Draft an Orbit task (title, description, acceptance criteria,
    /// context_files) for a reserved research item. Read-only: creates
    /// nothing, in the corpus or in Orbit.
    Plan {
        /// Path to the canonical research corpus.
        #[arg(long)]
        corpus: PathBuf,
        /// Drafting shape: one task owning a whole item (investigation), a
        /// disjoint code/artifacts unit (contribution), or a reconciling
        /// follow-up (synthesis).
        #[arg(long, value_enum)]
        shape: PlanShape,
        /// Reserved research record ID, such as R001.
        #[arg(long)]
        research_id: String,
        /// Question or outcome this work should address (investigation, contribution).
        #[arg(long)]
        objective: Option<String>,
        /// Contribution name used for its code and artifact paths (contribution).
        #[arg(long)]
        unit: Option<String>,
        /// Completed contribution name; repeat for multiple contributions (synthesis).
        #[arg(long = "contribution")]
        contributions: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum PlanShape {
    Investigation,
    Contribution,
    Synthesis,
}

#[derive(Debug, Args)]
pub(crate) struct ModeArg {
    /// Writer mode. `auto` detects it: primary checkout commits, a linked run
    /// worktree writes only its reserved R uncommitted. Naming a mode refuses
    /// the write when detection disagrees.
    #[arg(long, value_enum, default_value = "auto")]
    pub(crate) mode: WriterMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum WriterMode {
    Auto,
    Primary,
    Worktree,
}
