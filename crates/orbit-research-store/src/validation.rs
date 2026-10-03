//! Owner schema and whole-corpus graph validation.
//!
//! Problems are collected, never stopped at the first one, and worded for a
//! person: the file, the field, the offending value and what would be accepted.
//! Raw schema-validator output (instance dumps, regexes, `oneOf` talk) never
//! reaches a caller.
use crate::{Error, Result};
use jsonschema::{
    ValidationError,
    error::{TypeKind, ValidationErrorKind},
    paths::PathChunk,
    primitive_type::PrimitiveType,
};
use orbit_research_common::{CorpusIssue, Record};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const DATE_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";
/// Longest value echoed back inside a message.
const VALUE_WIDTH: usize = 60;

pub(crate) struct Contract {
    pub(crate) schema: Value,
    /// The whole contract; used only for a kind whose definition is not named.
    validator: jsonschema::JSONSchema,
    /// One validator per record kind, so a bad record is judged against its own
    /// kind's fields rather than reported as matching none of the four.
    kinds: BTreeMap<String, jsonschema::JSONSchema>,
    /// `data/manifest.json`, when the contract defines it.
    manifest: Option<jsonschema::JSONSchema>,
}

fn compile_schema(schema: &Value) -> Result<jsonschema::JSONSchema> {
    jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .compile(schema)
        .map_err(|e| Error::Invalid(format!("Invalid owner schema: {e}")))
}

impl Contract {
    pub(crate) fn compile(schema: Value) -> Result<Self> {
        let Some(declared) = schema["x-observatory"]["kinds"].as_object() else {
            return Err(Error::Invalid(
                "Corpus does not export the Observatory record contract".into(),
            ));
        };
        let validator = compile_schema(&schema)?;
        let mut kinds = BTreeMap::new();
        for (kind, spec) in declared {
            let Some(definition) = spec["def"].as_str() else {
                continue;
            };
            if !schema["$defs"][definition].is_object() {
                continue;
            }
            let mut own = schema.clone();
            if let Some(root) = own.as_object_mut() {
                root.remove("oneOf");
                root.insert("$ref".into(), json!(format!("#/$defs/{definition}")));
            }
            kinds.insert(kind.clone(), compile_schema(&own)?);
        }
        let manifest = if schema["$defs"]["data_manifest"].is_object() {
            let mut own = schema.clone();
            if let Some(root) = own.as_object_mut() {
                root.remove("oneOf");
                root.insert("$ref".into(), json!("#/$defs/data_manifest"));
            }
            Some(compile_schema(&own)?)
        } else {
            None
        };
        Ok(Self {
            schema,
            validator,
            kinds,
            manifest,
        })
    }

    /// Every way `metadata` breaks the contract for a record of `kind`.
    pub(crate) fn issues(&self, metadata: &Value, path: &str, kind: &str) -> Vec<CorpusIssue> {
        let validator = self.kinds.get(kind).unwrap_or(&self.validator);
        let Err(errors) = validator.validate(metadata) else {
            return Vec::new();
        };
        errors
            .flat_map(|error| self.describe(&error))
            .map(|(field, message)| CorpusIssue {
                path: path.to_owned(),
                field,
                message,
            })
            .collect()
    }

    /// Plain problems with a `data/manifest.json` value, or `None` when the
    /// contract does not define one.
    pub(crate) fn manifest_issues(
        &self,
        manifest: &Value,
    ) -> Option<Vec<(Option<String>, String)>> {
        let validator = self.manifest.as_ref()?;
        let Err(errors) = validator.validate(manifest) else {
            return Some(Vec::new());
        };
        Some(errors.flat_map(|error| self.describe(&error)).collect())
    }

    /// Validate a record read from the corpus: all problems, structured.
    pub(crate) fn validate(&self, metadata: &Value, path: &str, kind: &str) -> Result<()> {
        let issues = self.issues(metadata, path, kind);
        if issues.is_empty() {
            Ok(())
        } else {
            Err(Error::Corpus(issues))
        }
    }

    /// Validate a record the caller is about to write. A failure is the
    /// caller's input (a bad tag, a bad status), not a broken corpus.
    pub(crate) fn validate_input(&self, metadata: &Value, label: &str, kind: &str) -> Result<()> {
        let issues = self.issues(metadata, label, kind);
        if issues.is_empty() {
            return Ok(());
        }
        let lines: Vec<String> = issues
            .iter()
            .map(|issue| match &issue.field {
                Some(field) => format!("{field}: {}", issue.message),
                None => issue.message.clone(),
            })
            .collect();
        Err(Error::InvalidInput(lines.join("; ")))
    }

    /// Plain-words (field, message) pairs for one validator error. Most errors
    /// give one pair; an object with several unknown fields gives one each.
    fn describe(&self, error: &ValidationError<'_>) -> Vec<(Option<String>, String)> {
        let field = field_path(error);
        let value = show_value(&error.instance);
        let at = |field: &Option<String>, name: &str| match field {
            Some(parent) => format!("{parent}.{name}"),
            None => name.to_owned(),
        };
        let one = |field: Option<String>, message: String| vec![(field, message)];
        match &error.kind {
            ValidationErrorKind::Required { property } => {
                let name = property
                    .as_str()
                    .map_or_else(|| property.to_string(), str::to_owned);
                one(Some(at(&field, &name)), "required field is missing".into())
            }
            ValidationErrorKind::AdditionalProperties { unexpected }
            | ValidationErrorKind::UnevaluatedProperties { unexpected } => unexpected
                .iter()
                .map(|name| {
                    (
                        Some(at(&field, name)),
                        "is not a field of this kind of record; remove it".to_owned(),
                    )
                })
                .collect(),
            ValidationErrorKind::Enum { options } => {
                let allowed = match options.as_array() {
                    Some(options) => options
                        .iter()
                        .map(|option| {
                            option
                                .as_str()
                                .map_or_else(|| option.to_string(), str::to_owned)
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    None => options.to_string(),
                };
                one(
                    field,
                    format!("{value} is not allowed; allowed values: {allowed}"),
                )
            }
            ValidationErrorKind::Constant { expected_value } => one(
                field,
                format!("{value} is not allowed; the value must be {expected_value}"),
            ),
            ValidationErrorKind::Pattern { pattern } => {
                one(field, self.pattern_message(pattern, &value))
            }
            ValidationErrorKind::Format { format } if format == "date" => one(
                field,
                format!("{value} is not a date; expected YYYY-MM-DD, like 2026-01-31"),
            ),
            ValidationErrorKind::Format { format } => {
                one(field, format!("{value} is not a valid {format}"))
            }
            ValidationErrorKind::Type { kind } => {
                let expected = match kind {
                    TypeKind::Single(kind) => type_phrase(*kind).to_owned(),
                    TypeKind::Multiple(kinds) => kinds
                        .into_iter()
                        .map(type_phrase)
                        .collect::<Vec<_>>()
                        .join(" or "),
                };
                let found = type_phrase(PrimitiveType::from(error.instance.as_ref()));
                let message = if field.is_none() {
                    format!("the frontmatter must be a mapping of fields, not {found}")
                } else {
                    format!("must be {expected}, not {found}")
                };
                one(field, message)
            }
            ValidationErrorKind::MinLength { limit: 1 } => one(field, "must not be empty".into()),
            ValidationErrorKind::MinLength { limit } => {
                one(field, format!("must be at least {limit} characters"))
            }
            ValidationErrorKind::MaxLength { limit } => {
                one(field, format!("must be at most {limit} characters"))
            }
            ValidationErrorKind::MinItems { limit: 1 } => {
                one(field, "must list at least one item".into())
            }
            ValidationErrorKind::MinItems { limit } => {
                one(field, format!("must list at least {limit} items"))
            }
            ValidationErrorKind::MaxItems { limit } => {
                one(field, format!("must list at most {limit} items"))
            }
            ValidationErrorKind::Minimum { limit } => {
                one(field, format!("{value} is below the minimum of {limit}"))
            }
            ValidationErrorKind::Maximum { limit } => {
                one(field, format!("{value} is above the maximum of {limit}"))
            }
            ValidationErrorKind::OneOfNotValid | ValidationErrorKind::AnyOf => one(
                field,
                "does not match the fields of any kind of record".into(),
            ),
            _ => one(
                field,
                format!("{value} does not satisfy the corpus contract"),
            ),
        }
    }

    fn pattern_message(&self, pattern: &str, value: &str) -> String {
        let observatory = &self.schema["x-observatory"];
        if observatory["id_pattern"].as_str() == Some(pattern) {
            format!(
                "{value} is not a record id; expected an id like Q001 (a kind letter and three digits)"
            )
        } else if pattern == DATE_PATTERN {
            format!("{value} is not a date; expected YYYY-MM-DD, like 2026-01-31")
        } else if observatory["slug_pattern"].as_str() == Some(pattern) {
            format!(
                "{value} is not a slug; use lowercase letters, digits and single hyphens, like cache-p95"
            )
        } else {
            format!("{value} does not have the required format")
        }
    }
}

/// `tags[0]`, `assessments[1].verdict`; `None` for the whole frontmatter.
fn field_path(error: &ValidationError<'_>) -> Option<String> {
    let mut path = String::new();
    for chunk in &error.instance_path {
        match chunk {
            PathChunk::Property(name) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
            }
            PathChunk::Index(index) => path.push_str(&format!("[{index}]")),
            PathChunk::Keyword(_) => {}
        }
    }
    (!path.is_empty()).then_some(path)
}

fn type_phrase(kind: PrimitiveType) -> &'static str {
    match kind {
        PrimitiveType::Array => "a list",
        PrimitiveType::Boolean => "true or false",
        PrimitiveType::Integer => "a whole number",
        PrimitiveType::Null => "empty",
        PrimitiveType::Number => "a number",
        PrimitiveType::Object => "a mapping",
        PrimitiveType::String => "text",
    }
}

/// A value as it may appear inside a message: text is quoted and cut short,
/// a list or mapping is never dumped.
fn show_value(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let mut shown: String = text.chars().take(VALUE_WIDTH).collect();
            if text.chars().count() > VALUE_WIDTH {
                shown.push('…');
            }
            format!("\"{shown}\"")
        }
        Value::Array(_) => "the list".into(),
        Value::Object(_) => "the mapping".into(),
        Value::Null => "the empty value".into(),
        other => other.to_string(),
    }
}

pub(crate) fn validate_records(records: &BTreeMap<String, Record>) -> Result<()> {
    let mut issues = Vec::new();
    check_numbering(records, &mut issues);
    for record in records.values() {
        let issue = |field: &str, message: String| CorpusIssue {
            path: record.path.clone(),
            field: Some(field.to_owned()),
            message,
        };
        for (field, allowed) in [
            ("derived_from", &["Q", "H", "T", "R"] as &[&str]),
            ("answered_by", &["H", "R"]),
            ("tests", &["H"]),
            ("claims", &["H"]),
            ("supersedes", &["T"]),
        ] {
            if let Some(refs) = record.metadata[field].as_array() {
                for (index, target) in refs.iter().enumerate() {
                    let Some(target) = target.as_str() else {
                        issues.push(issue(
                            &format!("{field}[{index}]"),
                            "must be a record id like Q001".into(),
                        ));
                        continue;
                    };
                    let Some(target_record) = records.get(target) else {
                        issues.push(issue(
                            field,
                            format!("references {target}, which is not in the corpus"),
                        ));
                        continue;
                    };
                    if !allowed.contains(&target_record.kind.as_str()) {
                        issues.push(issue(
                            field,
                            format!(
                                "references {target}, a {} record; expected {}",
                                kind_name(&target_record.kind),
                                allowed.join(" or ")
                            ),
                        ));
                    }
                }
            }
        }
        if let Some(assessments) = record.metadata["assessments"].as_array() {
            for (index, assessment) in assessments.iter().enumerate() {
                let field = format!("assessments[{index}]");
                let Some(target) = assessment["research"].as_str() else {
                    issues.push(issue(
                        &format!("{field}.research"),
                        "must be a research id like R001".into(),
                    ));
                    continue;
                };
                match records.get(target) {
                    None => issues.push(issue(
                        &format!("{field}.research"),
                        format!("references {target}, which is not in the corpus"),
                    )),
                    Some(target_record) if target_record.kind != "R" => issues.push(issue(
                        &format!("{field}.research"),
                        format!(
                            "references {target}, a {} record; expected R",
                            kind_name(&target_record.kind)
                        ),
                    )),
                    Some(_) => {}
                }
                if let (Some(current), Some(revision)) = (
                    record.metadata["revision"].as_u64(),
                    assessment["revision"].as_u64(),
                ) && revision > current
                {
                    issues.push(issue(
                        &format!("{field}.revision"),
                        format!(
                            "is against revision {revision}, beyond the hypothesis's revision {current}"
                        ),
                    ));
                }
            }
        }
    }
    let mut done = BTreeSet::new();
    for id in records.keys() {
        check_lineage(id, records, &mut BTreeSet::new(), &mut done, &mut issues);
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::Corpus(issues))
    }
}

fn kind_name(kind: &str) -> &str {
    match kind {
        "Q" => "question",
        "H" => "hypothesis",
        "T" => "theory",
        "R" => "research",
        other => other,
    }
}

/// Ids run 001, 002, ... per kind. Only the first break in a kind is reported:
/// every later id then looks off by one, and saying so repeatedly buries the cause.
fn check_numbering(records: &BTreeMap<String, Record>, issues: &mut Vec<CorpusIssue>) {
    let mut numbers: BTreeMap<&str, Vec<(u32, &Record)>> = BTreeMap::new();
    for record in records.values() {
        match record
            .id
            .get(1..)
            .and_then(|digits| digits.parse::<u32>().ok())
        {
            Some(number) => numbers
                .entry(&record.kind)
                .or_default()
                .push((number, record)),
            None => issues.push(CorpusIssue {
                path: record.path.clone(),
                field: Some("id".into()),
                message: format!("{} is not a record id; expected an id like Q001", record.id),
            }),
        }
    }
    for (kind, mut values) in numbers {
        values.sort_unstable_by_key(|(number, _)| *number);
        for (index, (number, record)) in values.into_iter().enumerate() {
            let expected = index as u32 + 1;
            if number != expected {
                issues.push(CorpusIssue {
                    path: record.path.clone(),
                    field: Some("id".into()),
                    message: format!(
                        "{} leaves a gap: expected {kind}{expected:03} next (ids run from 001 with no gaps)",
                        record.id
                    ),
                });
                break;
            }
        }
    }
}

fn check_lineage(
    id: &str,
    records: &BTreeMap<String, Record>,
    visiting: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
    issues: &mut Vec<CorpusIssue>,
) {
    if done.contains(id) {
        return;
    }
    // A missing record was already reported as a bad reference.
    let Some(record) = records.get(id) else {
        return;
    };
    if !visiting.insert(id.into()) {
        issues.push(CorpusIssue {
            path: record.path.clone(),
            field: Some("derived_from".into()),
            message: format!("{id} derives from itself through a cycle of derived_from links"),
        });
        return;
    }
    for parent in record.metadata["derived_from"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        check_lineage(parent, records, visiting, done, issues);
    }
    visiting.remove(id);
    done.insert(id.into());
}
