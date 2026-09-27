//! Request contracts shared by deserialization and tool schema generation.
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    #[schemars(length(min = 1))]
    pub(super) request_key: String,
    pub(super) kind: RecordKind,
    #[schemars(length(min = 1))]
    pub(super) title: String,
    #[serde(default)]
    pub(super) body: String,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) derived_from: Vec<String>,
    /// Must be the kind's initial status (`planned` for R: the reservation).
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) mode: Option<Mode>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviseQuestion {
    #[schemars(length(min = 1))]
    pub(super) id: String,
    #[schemars(length(min = 1))]
    pub(super) expected_blob: String,
    #[schemars(length(min = 1))]
    pub(super) title: String,
    pub(super) body: String,
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) mode: Option<Mode>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    /// Question text; its first line becomes the title.
    #[schemars(length(min = 1))]
    pub(super) text: String,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    /// Retry key; defaults to a digest of the text and tags.
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub(super) request_key: Option<String>,
    #[serde(default)]
    pub(super) mode: Option<Mode>,
}

/// Omitted fields keep their current values.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Revise {
    #[schemars(regex(pattern = "^[QHTR][0-9]{3}$"))]
    pub(super) id: String,
    #[schemars(length(min = 1))]
    pub(super) expected_blob: String,
    #[schemars(length(min = 1))]
    pub(super) title: Option<String>,
    pub(super) body: Option<String>,
    pub(super) tags: Option<Vec<String>>,
    pub(super) status: Option<String>,
    /// Research only.
    pub(super) tests: Option<Vec<String>>,
    /// Research only.
    pub(super) orbit: Option<OrbitLink>,
    /// Research only: replacement data/manifest.json contents.
    pub(super) manifest: Option<Value>,
    #[serde(default)]
    pub(super) mode: Option<Mode>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct OrbitLink {
    pub(super) task: Option<String>,
    pub(super) run: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Assess {
    /// Hypothesis ID.
    #[schemars(regex(pattern = "^H[0-9]{3}$"))]
    pub(super) id: String,
    #[schemars(length(min = 1))]
    pub(super) expected_blob: String,
    /// Accepted research record the verdict rests on.
    #[schemars(regex(pattern = "^R[0-9]{3}$"))]
    pub(super) research: String,
    /// Hypothesis revision the verdict is about.
    #[schemars(range(min = 1))]
    pub(super) revision: u64,
    pub(super) verdict: Verdict,
    pub(super) strength: Strength,
    pub(super) note: Option<String>,
    #[serde(default)]
    pub(super) mode: Option<Mode>,
}

/// Always an explicit author choice; never derived from execution success.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Verdict {
    Supports,
    Refutes,
    Inconclusive,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Strength {
    Anecdote,
    Suggestive,
    Strong,
}

/// Expected writer mode; the write refuses when the checkout is in the other.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Primary,
    Worktree,
}

/// One tool, three drafting shapes. `shape` selects the variant; each carries
/// only the fields that shape needs.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Plan {
    Investigation {
        #[schemars(length(min = 1))]
        research_id: String,
        #[schemars(length(min = 1))]
        objective: String,
    },
    Contribution {
        #[schemars(length(min = 1))]
        research_id: String,
        #[schemars(length(min = 1))]
        unit: String,
        #[schemars(length(min = 1))]
        objective: String,
    },
    Synthesis {
        #[schemars(length(min = 1))]
        research_id: String,
        units: Vec<String>,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Show {
    #[schemars(regex(pattern = "^[QHTR][0-9]{3}$"))]
    pub(super) id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}

/// The kinds this application is authorized to create. Assessments stay explicit.
#[derive(Deserialize, JsonSchema)]
pub(super) enum RecordKind {
    Q,
    H,
    T,
    R,
}

impl Verdict {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Refutes => "refutes",
            Self::Inconclusive => "inconclusive",
        }
    }
}

impl Strength {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Anecdote => "anecdote",
            Self::Suggestive => "suggestive",
            Self::Strong => "strong",
        }
    }
}

impl RecordKind {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Q => "Q",
            Self::H => "H",
            Self::T => "T",
            Self::R => "R",
        }
    }
}
