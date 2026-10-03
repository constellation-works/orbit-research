//! Leaf contracts shared by persistence, application and presentation layers.
//! No filesystem operations, Git invocation, runtime, or workspace-crate dependencies.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    /// A write outside the writer's mode or scope: allocating or committing from a
    /// run worktree, or touching any record other than that worktree's reserved R.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Internal(String),
    /// `assess` could not verify that the cited research result was accepted.
    #[error(transparent)]
    Acceptance(#[from] AcceptanceFailure),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
}

/// Why `assess` refused a research result for lack of verifiable acceptance.
/// Each variant names the next step, so a caller never has to guess whether to
/// accept, re-accept or fix the environment. None of them is ever downgraded to
/// an accepted result.
#[derive(Debug, thiserror::Error)]
pub enum AcceptanceFailure {
    #[error(
        "{research} has no `orbit.task` in its frontmatter, so its acceptance cannot be found; link the record to its Orbit task and run the plugin's `accept` tool before assessing"
    )]
    NoTask { research: String },
    #[error(
        "task {task} has no `research-acceptance.json` for {research}; run the plugin's `accept` tool on that task before assessing"
    )]
    Missing { research: String, task: String },
    #[error(
        "Orbit could not provide the acceptance for {research} from task {task}: {reason}; run from the corpus checkout with Orbit installed (ORBIT_BIN or PATH) and the workspace registered, then retry"
    )]
    Unreachable {
        research: String,
        task: String,
        reason: String,
    },
    #[error(
        "task {task}'s `research-acceptance.json` for {research} is unreadable: {reason}; re-run the plugin's `accept` tool or inspect the artifact"
    )]
    Malformed {
        research: String,
        task: String,
        reason: String,
    },
    #[error(
        "task {task}'s `research-acceptance.json` accepts {found}, not {research}; the record's `orbit.task` points at the wrong task"
    )]
    WrongResearch {
        research: String,
        task: String,
        found: String,
    },
    #[error(
        "{research} changed after task {task} accepted it (accepted blob {accepted}, current blob {current}); validate and accept the current result again before assessing"
    )]
    StaleBlob {
        research: String,
        task: String,
        accepted: String,
        current: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// A disposable view of a canonical Markdown file. No scientific state is stored here.
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub metadata: Value,
    pub body: String,
    pub content_sha256: String,
    pub git_blob: String,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub revision: String,
    pub records: Vec<Record>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Reservation {
    pub id: String,
    pub path: String,
    pub commit: String,
    pub request_digest: String,
    /// Git blob of the written record: the `expected_blob` for its next revision.
    /// Receipts stored before this field existed are filled in when read back.
    #[serde(default)]
    pub git_blob: String,
    /// Present (true) only when an identical retry returned the receipt of an
    /// earlier, already committed write and changed nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
}
