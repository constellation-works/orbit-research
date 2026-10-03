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
    /// The corpus holds records that do not satisfy its contract. Every problem
    /// found is carried so a caller can fix them in one pass; the text form is
    /// capped, the structured form is not.
    #[error("{}", render_issues(.0))]
    Corpus(Vec<CorpusIssue>),
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

/// One problem in one corpus file, in plain words. `field` names the
/// frontmatter field when the problem is about one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusIssue {
    pub path: String,
    pub field: Option<String>,
    pub message: String,
}

impl std::fmt::Display for CorpusIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.field {
            Some(field) => write!(formatter, "{}: {field}: {}", self.path, self.message),
            None => write!(formatter, "{}: {}", self.path, self.message),
        }
    }
}

/// How many problems the text form of a corpus error lists before `+N more`.
pub const ISSUE_DISPLAY_LIMIT: usize = 20;

/// One line for a single problem; otherwise a count and the first
/// [`ISSUE_DISPLAY_LIMIT`] problems, one per line, then `+N more`.
pub fn render_issues(issues: &[CorpusIssue]) -> String {
    render_issues_with(issues, "orbit-research research check")
}

/// [`render_issues`] with the command a caller should run again after fixing
/// the problems, so a CLI can name the corpus it was given.
pub fn render_issues_with(issues: &[CorpusIssue], rerun: &str) -> String {
    if let [only] = issues {
        return only.to_string();
    }
    let mut text = format!("{} problems in the corpus:", issues.len());
    for issue in issues.iter().take(ISSUE_DISPLAY_LIMIT) {
        text.push('\n');
        text.push_str(&issue.to_string());
    }
    if issues.len() > ISSUE_DISPLAY_LIMIT {
        text.push_str(&format!(
            "\n+{} more; fix these and run `{rerun}` again",
            issues.len() - ISSUE_DISPLAY_LIMIT
        ));
    }
    text
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
        "{research} changed after task {task} accepted it (accepted blob {accepted}, current blob {current}); accept refuses different evidence for a task that already has some, so reserve a new research record derived from it with `research create --kind R --status planned --derived-from {research}` and assess against that"
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
    /// Present (false) only when the write left the record exactly as it was,
    /// so no commit was made. Absent means the write changed the corpus.
    #[serde(default = "changed_by_default", skip_serializing_if = "is_changed")]
    pub changed: bool,
}

fn changed_by_default() -> bool {
    true
}

fn is_changed(changed: &bool) -> bool {
    *changed
}
