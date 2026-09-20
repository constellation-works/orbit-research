use std::path::PathBuf;

use crate::output::OutputMode;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "orbit-research",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub(crate) struct Cli {
    /// Output mode.
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
    /// Read fresh Orbit task/run evidence for a linked request.
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
