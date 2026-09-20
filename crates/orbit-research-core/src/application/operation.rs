//! One registry binds wire names, request schemas, descriptions and handlers.
use super::{api, request::*};
use crate::{Application, Error, Result};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{str::FromStr, sync::OnceLock};

#[derive(Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: schemars::schema::RootSchema,
}

// Keep the registry declarative: adding an operation binds its request type and
// handler here, so discovery cannot drift from dispatch or wire-name parsing.
macro_rules! operations {
    ($($variant:ident => ($name:literal, $request:ty, $handler:ident, $description:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Operation { $($variant),+ }

        impl Operation {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }

            pub fn definition(self) -> ToolDefinition {
                match self {
                    $(Self::$variant => ToolDefinition {
                        name: $name,
                        description: $description,
                        input_schema: schemars::schema_for!($request),
                    }),+
                }
            }

            pub(super) fn execute(self, app: &Application, input: Value) -> Result<Value> {
                match self {
                    $(Self::$variant => {
                        // Compile once per operation. Parse before schema validation
                        // to preserve useful Serde unknown-field/type diagnostics.
                        static VALIDATOR: OnceLock<std::result::Result<jsonschema::JSONSchema, String>> = OnceLock::new();
                        let request = decode::<$request>(input, &VALIDATOR)?;
                        api::$handler(app, request)
                    }),+
                }
            }
        }

        impl FromStr for Operation {
            type Err = Error;

            fn from_str(name: &str) -> Result<Self> {
                match name {
                    $($name => Ok(Self::$variant)),+,
                    _ => Err(Error::InvalidInput(format!("Unknown research operation: {name}"))),
                }
            }
        }
    };
}

operations! {
    Backend => (
        "research.backend",
        Empty,
        backend,
        "Inspect the configured Orbit backend and compatibility. Never changes backend scope."
    ),
    WorkLinks => (
        "research.work_links",
        Empty,
        work_links,
        "List local request correlations pointing to authoritative Orbit tasks. Cached pointers are not fresh run status."
    ),
    WorkStatus => (
        "research.work_status",
        RequestKey,
        work_status,
        "Read fresh Orbit task/run evidence for a linked request."
    ),
    Promote => (
        "research.promote",
        RequestKey,
        promote,
        "Explicitly approve the linked task for execution. Requires configured backend authority; does not dispatch."
    ),
    Dispatch => (
        "research.dispatch",
        Dispatch,
        dispatch,
        "Explicitly dispatch an approved linked task. Unknown prior submissions are reconciled, never blindly repeated."
    ),
    Cancel => (
        "research.cancel",
        RequestKey,
        cancel,
        "Explicitly cancel only the run currently correlated with the linked task."
    ),
    ValidateResult => (
        "research.validate_result",
        ValidateResult,
        validate_result,
        "Fetch an Orbit receipt and verify its task/run, published record and artifact identities. Does not infer scientific support."
    ),
    LinkWork => (
        "research.link_work",
        LinkWork,
        link_work,
        "Create an Orbit task from a validated work plan. Persist request_key for reconciliation. Does not dispatch."
    ),
    List => (
        "research.list",
        Empty,
        list,
        "Read the validated canonical research corpus and tags."
    ),
    Check => (
        "research.check",
        Empty,
        list,
        "Read and validate the canonical corpus without changing records."
    ),
    Create => (
        "research.create",
        Create,
        create,
        "Reserve and commit a canonical Q/H/T/R record on the integration checkout. Retain request_key across retries; never allocate IDs in worker worktrees. Does not dispatch any agent."
    ),
    ReviseQuestion => (
        "research.revise_question",
        ReviseQuestion,
        revise_question,
        "Commit a question revision only if expected_blob still matches. Frozen path and lineage are preserved; requires clean primary checkout."
    ),
    PlanInvestigation => (
        "research.plan_investigation",
        Investigation,
        investigation,
        "Plan a single task owning one reserved research item, including its canonical result and evidence. Does not dispatch."
    ),
    PlanContribution => (
        "research.plan_contribution",
        Contribution,
        contribution,
        "Plan disjoint code and artifact paths for one contribution to an existing research item. Use returned context_files on the Orbit task; shared summary is read-only."
    ),
    PlanSynthesis => (
        "research.plan_synthesis",
        Synthesis,
        synthesis,
        "Plan a follow-up Orbit task to reconcile contributions into the shared research README and input manifest. Schedule after contributing tasks deliver."
    ),
}

fn decode<T: DeserializeOwned + JsonSchema>(
    input: Value,
    validator: &OnceLock<std::result::Result<jsonschema::JSONSchema, String>>,
) -> Result<T> {
    let request = serde_json::from_value(input.clone())
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let validator = validator.get_or_init(|| {
        let schema =
            serde_json::to_value(schemars::schema_for!(T)).map_err(|error| error.to_string())?;
        jsonschema::JSONSchema::compile(&schema).map_err(|error| error.to_string())
    });
    let validator = validator
        .as_ref()
        .map_err(|error| Error::Internal(format!("Invalid built-in request schema: {error}")))?;
    if let Err(mut errors) = validator.validate(&input) {
        let message = errors
            .next()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "Invalid operation arguments".into());
        return Err(Error::InvalidInput(message));
    }
    Ok(request)
}
