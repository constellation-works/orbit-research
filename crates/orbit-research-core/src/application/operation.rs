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
    WorkLinks => (
        "research.work_links",
        Empty,
        work_links,
        "List local request correlations pointing to authoritative Orbit tasks. Cached pointers are not fresh run status."
    ),
    List => (
        "research.list",
        Empty,
        list,
        "Read the validated canonical research corpus and tags."
    ),
    Show => (
        "research.show",
        Show,
        show,
        "Read one canonical record by id, with its body and lineage. Refuses an id absent from the corpus."
    ),
    Check => (
        "research.check",
        Empty,
        check,
        "Validate the canonical corpus and report its base revision and record/tag counts without returning record bodies. Does not change records."
    ),
    Create => (
        "research.create",
        Create,
        create,
        "Reserve and commit a canonical Q/H/T/R record on the primary checkout (primary mode). An R starts planned: that is the reservation. Retain request_key across retries; refused in a run worktree. Does not dispatch any agent."
    ),
    Capture => (
        "research.capture",
        Capture,
        capture,
        "Capture a new question from text and tags alone and commit it on the primary checkout. No task is needed. An identical retry returns the same question."
    ),
    Revise => (
        "research.revise",
        Revise,
        revise,
        "Revise a record only if expected_blob still matches. Primary mode commits Q/H/T edits; a hypothesis statement change bumps its revision. Worktree mode writes only the run's reserved R (README and data/manifest.json) and does not commit. The result names the mode."
    ),
    Assess => (
        "research.assess",
        Assess,
        assess,
        "Append one explicit verdict to a hypothesis's assessments against an existing revision and an accepted research record. Append-only; status follows the owner schema's verdict_status for the current revision. Execution success is never support. Primary mode only."
    ),
    ReviseQuestion => (
        "research.revise_question",
        ReviseQuestion,
        revise_question,
        "Commit a question revision only if expected_blob still matches. Frozen path and lineage are preserved; requires clean primary checkout."
    ),
    Plan => (
        "research.plan",
        Plan,
        plan,
        "Draft an Orbit task (title, description, acceptance_criteria, context_files naming the reserved research item) for a Q/H: investigation (one task owns a whole reserved research item), contribution (disjoint per-unit code/artifacts paths) or synthesis (reconciling completed contributions). Read-only; does not create or dispatch anything."
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
