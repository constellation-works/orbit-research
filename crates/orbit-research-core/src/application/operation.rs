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
        "Revise a record only if expected_blob still matches. Omitted fields keep their current values; tags: [] clears the tags; a hypothesis or theory body cannot be set empty. Primary mode commits Q/H/T edits; a hypothesis statement change bumps its revision. Worktree mode writes only the run's reserved R (README and data/manifest.json) and does not commit. The result names the mode."
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
        "Commit a question revision only if expected_blob still matches. Omitted title, body and tags keep their current values (tags: [] clears them); give at least one. Frozen path and lineage are preserved; requires clean primary checkout."
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
    if let Err(errors) = validator.validate(&input) {
        let mut unresolved_variant = false;
        let mut messages: Vec<String> = errors
            .map(|error| {
                unresolved_variant |= (&error.instance_path).into_iter().next().is_none()
                    && matches!(
                        error.kind,
                        jsonschema::error::ValidationErrorKind::OneOfNotValid
                            | jsonschema::error::ValidationErrorKind::AnyOf
                    );
                input_message(&error)
            })
            .collect();
        // A tagged union reports only that no variant matched; name the field
        // that broke the variant the caller chose.
        if unresolved_variant && let Some(specific) = variant_messages::<T>(&input) {
            messages = specific;
        }
        messages.dedup();
        if messages.is_empty() {
            messages.push("Invalid operation arguments".into());
        }
        return Err(Error::InvalidInput(messages.join("; ")));
    }
    Ok(request)
}

/// The input-schema failures of the one variant of a tagged union (such as
/// `plan`'s `shape`) that the input selects, in plain words. `None` when the
/// schema is not a union or the input selects no variant.
fn variant_messages<T: JsonSchema>(input: &Value) -> Option<Vec<String>> {
    let schema = serde_json::to_value(schemars::schema_for!(T)).ok()?;
    let mut variant = schema["oneOf"]
        .as_array()?
        .iter()
        .find(|variant| {
            variant["properties"].as_object().is_some_and(|fields| {
                fields.iter().any(|(name, field)| {
                    let chosen = input.get(name);
                    chosen.is_some()
                        && field["enum"].as_array().is_some_and(|options| {
                            options.len() == 1 && Some(&options[0]) == chosen
                        })
                })
            })
        })?
        .clone();
    if let Some(definitions) = schema.get("definitions") {
        variant["definitions"] = definitions.clone();
    }
    let validator = jsonschema::JSONSchema::compile(&variant).ok()?;
    let errors = validator.validate(input).err()?;
    Some(errors.map(|error| input_message(&error)).collect())
}

/// One input-schema failure in plain words, naming the argument. Raw validator
/// text quotes patterns and instance dumps, which tell a caller nothing to fix.
fn input_message(error: &jsonschema::ValidationError<'_>) -> String {
    use jsonschema::{error::ValidationErrorKind as Kind, paths::PathChunk};
    let mut field = String::new();
    for chunk in &error.instance_path {
        match chunk {
            PathChunk::Property(name) => {
                if !field.is_empty() {
                    field.push('.');
                }
                field.push_str(name);
            }
            PathChunk::Index(index) => field.push_str(&format!("[{index}]")),
            PathChunk::Keyword(_) => {}
        }
    }
    let name = if field.is_empty() {
        "the input".to_owned()
    } else {
        format!("`{field}`")
    };
    let value = match error.instance.as_ref() {
        Value::String(text) => format!("\"{}\"", text.chars().take(60).collect::<String>()),
        Value::Array(_) | Value::Object(_) => "that value".to_owned(),
        other => other.to_string(),
    };
    match &error.kind {
        Kind::MinLength { limit: 1 } => format!("{name} is required and must not be empty"),
        Kind::MinLength { limit } => format!("{name} must be at least {limit} characters"),
        Kind::MaxLength { limit } => format!("{name} must be at most {limit} characters"),
        Kind::Pattern { pattern } => format!("{name} {value} {}", pattern_expectation(pattern)),
        Kind::Required { property } => {
            format!("`{}` is required", property.as_str().unwrap_or("a field"))
        }
        Kind::Enum { options } => {
            let allowed = options
                .as_array()
                .map(|options| {
                    options
                        .iter()
                        .map(|option| {
                            option
                                .as_str()
                                .map_or_else(|| option.to_string(), str::to_owned)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("{name} {value} is not allowed; allowed values: {allowed}")
        }
        Kind::Minimum { limit } => format!("{name} must be at least {limit}"),
        Kind::Maximum { limit } => format!("{name} must be at most {limit}"),
        Kind::Type { .. } => format!("{name} has the wrong type"),
        Kind::AdditionalProperties { unexpected } => format!(
            "unknown field{} {}",
            if unexpected.len() == 1 { "" } else { "s" },
            unexpected
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => format!("{name} is not valid"),
    }
}

/// What an id-shaped pattern asks for, without printing the regular expression.
fn pattern_expectation(pattern: &str) -> String {
    let kind = |letter: &str| match letter {
        "Q" => Some("question"),
        "H" => Some("hypothesis"),
        "T" => Some("theory"),
        "R" => Some("research"),
        _ => None,
    };
    if pattern == "^[QHTR][0-9]{3}$" {
        return "is not a record id; expected an id like Q001".to_owned();
    }
    if let Some(letter) = pattern
        .strip_prefix('^')
        .and_then(|rest| rest.strip_suffix("[0-9]{3}$"))
        && let Some(name) = kind(letter)
    {
        return format!("is not a {name} id; expected an id like {letter}001");
    }
    "is not in the expected format".to_owned()
}
