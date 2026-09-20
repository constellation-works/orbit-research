//! Request contracts shared by deserialization and tool schema generation.
use super::work::WorkPlan;
use schemars::JsonSchema;
use serde::Deserialize;

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
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Investigation {
    #[schemars(length(min = 1))]
    pub(super) research_id: String,
    #[schemars(length(min = 1))]
    pub(super) objective: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Contribution {
    #[schemars(length(min = 1))]
    pub(super) research_id: String,
    #[schemars(length(min = 1))]
    pub(super) unit: String,
    #[schemars(length(min = 1))]
    pub(super) objective: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Synthesis {
    #[schemars(length(min = 1))]
    pub(super) research_id: String,
    pub(super) units: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkWork {
    #[schemars(length(min = 1))]
    pub(super) request_key: String,
    #[schemars(length(min = 1))]
    pub(super) title: String,
    #[schemars(length(min = 1))]
    pub(super) crew: String,
    pub(super) plan: WorkPlan,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestKey {
    #[schemars(length(min = 1))]
    pub(super) request_key: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Dispatch {
    #[schemars(length(min = 1))]
    pub(super) request_key: String,
    #[schemars(length(min = 1))]
    pub(super) base: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ValidateResult {
    #[schemars(length(min = 1))]
    pub(super) request_key: String,
    #[schemars(length(min = 1))]
    pub(super) receipt_path: String,
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
