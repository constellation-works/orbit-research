//! Pure scientific records, validation, reconciliation, and v1/v2 identity.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::str::FromStr;
use std::sync::OnceLock;

use jsonschema::{Draft, JSONSchema};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const NUMBER_TOKEN: &str = "$serde_json::private::Number";
const RECORD_KINDS: &[&str] = &[
    "program",
    "claim",
    "protocol",
    "experiment",
    "artifact",
    "assessment",
];

const SCHEMAS: &[(&str, &str)] = &[
    (
        "urn:orbit-research:schema:v1:record",
        include_str!("../../../src/orbit_research/schemas/v1/record.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:manifest",
        include_str!("../../../src/orbit_research/schemas/v1/manifest.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:import-report",
        include_str!("../../../src/orbit_research/schemas/v1/import-report.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:program",
        include_str!("../../../src/orbit_research/schemas/v1/program.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:claim",
        include_str!("../../../src/orbit_research/schemas/v1/claim.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:protocol",
        include_str!("../../../src/orbit_research/schemas/v1/protocol.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:experiment",
        include_str!("../../../src/orbit_research/schemas/v1/experiment.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:artifact",
        include_str!("../../../src/orbit_research/schemas/v1/artifact.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v1:assessment",
        include_str!("../../../src/orbit_research/schemas/v1/assessment.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v2:record",
        include_str!("../../../src/orbit_research/schemas/v2/record.schema.json"),
    ),
    (
        "urn:orbit-research:schema:v2:export",
        include_str!("../../../src/orbit_research/schemas/v2/export.schema.json"),
    ),
];

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("duplicate object key: {0}")]
    DuplicateKey(String),
    #[error("canonical JSON does not support non-finite numbers")]
    NonFiniteNumber,
    #[error("expected {0}")]
    Expected(&'static str),
    #[error("invalid repository id: {0}")]
    RepositoryId(String),
    #[error("invalid research URN: {0}")]
    RecordUrn(String),
    #[error("invalid SHA-256 digest: {0}")]
    Digest(String),
    #[error("invalid Git revision: {0}")]
    GitRevision(String),
    #[error("schema registry is invalid: {0}")]
    Schema(String),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(String);

impl FromStr for RepositoryId {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut chars = value.chars();
        let first = chars.next();
        if first.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContractError::RepositoryId(value.to_owned()))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordUrn(String);

impl FromStr for RecordUrn {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let pieces: Vec<_> = value.splitn(5, ':').collect();
        if pieces.len() == 5
            && pieces[0] == "urn"
            && pieces[1] == "research"
            && pieces[2].parse::<RepositoryId>().is_ok()
            && RECORD_KINDS.contains(&pieces[3])
            && !pieces[4].is_empty()
            && !pieces[4].chars().any(char::is_whitespace)
            && !pieces[4].contains('/')
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContractError::RecordUrn(value.to_owned()))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Digest(String);

pub type RevisionId = Digest;

impl FromStr for Digest {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if valid_prefixed_hex(value, "sha256:", 64) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContractError::Digest(value.to_owned()))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitRevision(String);

impl FromStr for GitRevision {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if (value.len() == 40 || value.len() == 64)
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContractError::GitRevision(value.to_owned()))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceStatus {
    Pending,
    Resolved,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub repository: RepositoryId,
    pub id: RecordUrn,
    pub revision_id: RevisionId,
    pub source_revision: Option<GitRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    #[serde(flatten)]
    pub pin: Pin,
    pub status: ReferenceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPin(pub Pin);

/// Verification is supplied by an I/O crate; the contract never opens files or performs network I/O.
pub trait ArtifactResolver {
    fn verify(
        &self,
        record: &Value,
        reference: &Reference,
    ) -> Result<Option<VerifiedPin>, ContractError>;
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $value)] $variant),+ }
    };
}

string_enum!(Kind { Program => "program", Claim => "claim", Protocol => "protocol", Experiment => "experiment", Artifact => "artifact", Assessment => "assessment" });
string_enum!(Activity { Active => "active", Paused => "paused", Retired => "retired", Resolved => "resolved", Unknown => "unknown" });
string_enum!(Scope { Derivation => "derivation", SimulationUnderAssumptions => "simulation-under-assumptions", SyntheticCalibration => "synthetic-calibration", Observation => "observation", Literature => "literature", Unknown => "unknown" });
string_enum!(Verdict { Supported => "supported", Refuted => "refuted", Inconclusive => "inconclusive", Conditional => "conditional", Untested => "untested", Unknown => "unknown" });
string_enum!(Inference { ConfirmatoryPrimary => "confirmatory-primary", Historical => "historical", Exploratory => "exploratory" });
string_enum!(Freeze { Prospective => "prospective", HistoricalUnverified => "historical-unverified", Registered => "registered" });
string_enum!(ExecutionStatus { Running => "running", Completed => "completed", Failed => "failed", Unknown => "unknown" });
string_enum!(EvidenceSummary { Supports => "supports", Refutes => "refutes", Mixed => "mixed", Inconclusive => "inconclusive", Unmeasured => "unmeasured" });

fn valid_prefixed_hex(value: &str, prefix: &str, digits: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|tail| {
        tail.len() == digits
            && tail
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Parse strict JSON while preserving arbitrarily large integer text and rejecting duplicate keys.
pub fn parse_json(data: &[u8]) -> Result<Value, ContractError> {
    let mut deserializer = serde_json::Deserializer::from_slice(data);
    let value = UniqueValue.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

struct UniqueValue;

impl<'de> DeserializeSeed<'de> for UniqueValue {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E>(self, value: f64) -> Result<Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E>(self, value: &str) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(UniqueValue)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let Some(first_key) = object.next_key::<String>()? else {
            return Ok(Value::Object(Map::new()));
        };
        if first_key == NUMBER_TOKEN {
            let encoded = object.next_value::<String>()?;
            let number = Number::from_str(&encoded).map_err(de::Error::custom)?;
            return Ok(Value::Number(number));
        }
        let mut values = Map::new();
        let first_value = object.next_value_seed(UniqueValue)?;
        values.insert(first_key, first_value);
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate object key: {key}")));
            }
            let value = object.next_value_seed(UniqueValue)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

/// Encode exactly like CPython `json.dumps(sort_keys=True, separators=(',', ':'), ensure_ascii=False)`.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>, ContractError> {
    let mut output = String::new();
    write_canonical(value, &mut output)?;
    Ok(output.into_bytes())
}

fn write_canonical(value: &Value, output: &mut String) -> Result<(), ContractError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&python_number(value)?),
        Value::String(value) => output.push_str(&serde_json::to_string(value)?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_canonical(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let sorted: BTreeMap<_, _> = values.iter().collect();
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key)?);
                output.push(':');
                write_canonical(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn python_number(number: &Number) -> Result<String, ContractError> {
    let raw = number.to_string();
    if !raw.contains(['.', 'e', 'E']) {
        return Ok(raw);
    }
    let value = number.as_f64().ok_or(ContractError::NonFiniteNumber)?;
    if !value.is_finite() {
        return Err(ContractError::NonFiniteNumber);
    }
    let mut buffer = ryu::Buffer::new();
    Ok(pythonize_float(buffer.format_finite(value)))
}

fn pythonize_float(shortest: &str) -> String {
    let (negative, unsigned) = shortest
        .strip_prefix('-')
        .map_or((false, shortest), |s| (true, s));
    let exponent_form = unsigned.split_once(['e', 'E']);
    let (mantissa, supplied_exp) = exponent_form.map_or((unsigned, 0_i32), |(m, e)| {
        (m, e.parse::<i32>().unwrap_or(0))
    });
    let dot = mantissa.find('.').unwrap_or(mantissa.len());
    let raw_digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let Some(first_nonzero) = raw_digits.find(|c| c != '0') else {
        return if negative { "-0.0" } else { "0.0" }.to_owned();
    };
    let digits = raw_digits[first_nonzero..].trim_end_matches('0').to_owned();
    let decimal_exp = supplied_exp + i32::try_from(dot).unwrap_or(i32::MAX)
        - i32::try_from(first_nonzero).unwrap_or(i32::MAX)
        - 1;
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&decimal_exp) {
        let decimal_at = decimal_exp + 1;
        let mut rendered = String::from(sign);
        if decimal_at <= 0 {
            rendered.push_str("0.");
            for _ in 0..-decimal_at {
                rendered.push('0');
            }
            rendered.push_str(&digits);
        } else if usize::try_from(decimal_at).is_ok_and(|at| at < digits.len()) {
            let at = usize::try_from(decimal_at).unwrap_or(0);
            rendered.push_str(&digits[..at]);
            rendered.push('.');
            rendered.push_str(&digits[at..]);
        } else {
            rendered.push_str(&digits);
            let zeroes = usize::try_from(decimal_at)
                .unwrap_or(digits.len())
                .saturating_sub(digits.len());
            for _ in 0..zeroes {
                rendered.push('0');
            }
            rendered.push_str(".0");
        }
        rendered
    } else {
        let mut rendered = String::from(sign);
        rendered.push(digits.chars().next().unwrap_or('0'));
        if digits.len() > 1 {
            rendered.push('.');
            rendered.push_str(&digits[1..]);
        }
        let absolute_exp = decimal_exp.unsigned_abs();
        let _ = write!(
            rendered,
            "e{}{absolute_exp:02}",
            if decimal_exp >= 0 { '+' } else { '-' },
        );
        rendered
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn protocol_digest(semantic: &Value) -> Result<String, ContractError> {
    Ok(digest_bytes(&canonical_json(semantic)?))
}

pub fn revision_digest(record: &Value) -> Result<String, ContractError> {
    let object = record
        .as_object()
        .ok_or(ContractError::Expected("a record object"))?;
    if object.get("kind").and_then(Value::as_str) == Some("protocol") {
        let semantic = object
            .get("payload")
            .and_then(|v| v.get("semantic"))
            .ok_or(ContractError::Expected("payload.semantic"))?;
        return protocol_digest(semantic);
    }
    let mut identity = object.clone();
    identity.remove("revision_id");
    identity.remove("presentation");
    identity.remove("provenance");
    Ok(digest_bytes(&canonical_json(&Value::Object(identity))?))
}

pub fn validate(document: &Value, targets: &[Value]) -> Vec<String> {
    let mut errors = schema_errors(document);
    if !errors.is_empty() {
        return errors;
    }
    let kind = text(document, "kind").unwrap_or_default();
    if kind == "export" {
        if let Some(records) = array(document, "records") {
            for (index, record) in records.iter().enumerate() {
                let context: Vec<_> = records
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, v)| v.clone())
                    .collect();
                errors.extend(validate(record, &context));
            }
            if let Some(manifests) = array(document, "manifests") {
                for manifest in manifests {
                    errors.extend(validate(manifest, records));
                }
                let pinned: BTreeSet<_> = manifests
                    .iter()
                    .flat_map(|manifest| array(manifest, "references").into_iter().flatten())
                    .filter_map(reference_pin)
                    .collect();
                let expected: BTreeSet<_> = records.iter().filter_map(full_pin).collect();
                if pinned != expected {
                    errors.push(
                        "export manifests must account for every exact record snapshot".to_owned(),
                    );
                }
            }
        }
        return errors;
    }

    let records: Vec<&Value> = if kind == "import-report" {
        array(document, "candidates").map_or_else(Vec::new, |values| values.iter().collect())
    } else if kind == "manifest" {
        Vec::new()
    } else {
        vec![document]
    };

    let mut known: HashMap<RecordKey, &Value> = HashMap::new();
    for target in targets {
        if !RECORD_KINDS.contains(&text(target, "kind").unwrap_or_default()) {
            errors.push("target must be an individual scientific record".to_owned());
        } else {
            let target_errors = validate_structural(target);
            if target_errors.is_empty() {
                if let Some(key) = record_key(target)
                    && known.insert(key.clone(), target).is_some()
                {
                    errors.push(format!("ambiguous target: {key:?}"));
                }
            } else {
                errors.extend(target_errors.into_iter().map(|e| format!("target: {e}")));
            }
        }
    }
    let mut aliases = HashMap::new();
    for record in &records {
        let Some(key) = record_key(record) else {
            continue;
        };
        if known.insert(key.clone(), record).is_some() {
            errors.push(format!("duplicate identity/revision: {key:?}"));
        }
        validate_record(record, &mut errors, &mut aliases);
    }
    if kind == "import-report" {
        validate_import_report(document, &records, &mut errors);
    }

    let mut references: Vec<&Value> = Vec::new();
    if kind == "manifest" {
        references.extend(array(document, "references").into_iter().flatten());
    } else {
        for record in &records {
            references.extend(record_references(record));
        }
        if kind == "import-report" {
            references.extend(
                document
                    .get("manifest")
                    .and_then(|v| array(v, "references"))
                    .into_iter()
                    .flatten(),
            );
        }
    }
    validate_manifest(document, kind, &mut errors);
    for reference in references {
        validate_reference(reference, &known, &records, &mut errors);
    }
    errors
}

/// Schema and per-record invariants only: no reference resolution against a closure.
/// The owner crate uses this to check one canonical append it has just read from disk.
pub fn validate_structural(record: &Value) -> Vec<String> {
    let mut errors = schema_errors(record);
    if errors.is_empty() {
        validate_record(record, &mut errors, &mut HashMap::new());
    }
    errors
}

/// The embedded schemas, parsed and compiled once. Validation runs on every record of
/// every closure, so recompiling this registry per call would dominate an owner append.
fn registry() -> &'static Result<HashMap<&'static str, JSONSchema>, String> {
    static REGISTRY: OnceLock<Result<HashMap<&'static str, JSONSchema>, String>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut parsed = HashMap::new();
        for (schema_id, source) in SCHEMAS {
            let schema = serde_json::from_str::<Value>(source)
                .map_err(|error| format!("embedded schema is invalid: {error}"))?;
            parsed.insert(*schema_id, schema);
        }
        let mut compiled = HashMap::new();
        for (schema_id, schema) in &parsed {
            let mut options = JSONSchema::options();
            options.with_draft(Draft::Draft202012);
            for (other, document) in &parsed {
                options.with_document((*other).to_owned(), document.clone());
            }
            let validator = options
                .compile(schema)
                .map_err(|error| format!("embedded schema is invalid: {error}"))?;
            compiled.insert(*schema_id, validator);
        }
        Ok(compiled)
    })
}

fn schema_errors(document: &Value) -> Vec<String> {
    let Some(object) = document.as_object() else {
        return vec!["$: expected an object".to_owned()];
    };
    let version = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = if kind == "export" {
        "export"
    } else if kind == "manifest" {
        "manifest"
    } else if kind == "import-report" {
        "import-report"
    } else {
        "record"
    };
    let id = format!("urn:orbit-research:schema:v{version}:{name}");
    let compiled = match registry() {
        Ok(compiled) => compiled,
        Err(message) => return vec![message.clone()],
    };
    let Some(validator) = compiled.get(id.as_str()) else {
        return vec!["unsupported schema version or document kind".to_owned()];
    };
    match validator.validate(document) {
        Ok(()) => Vec::new(),
        Err(found) => found
            .map(|error| {
                let path = if error.instance_path.to_string().is_empty() {
                    "$".to_owned()
                } else {
                    error.instance_path.to_string()
                };
                format!("{path}: {error}")
            })
            .collect(),
    }
}

type RecordKey = (String, String, Option<String>);

fn record_key(record: &Value) -> Option<RecordKey> {
    Some((
        text(record, "id")?.to_owned(),
        text(record, "revision_id")?.to_owned(),
        record
            .get("provenance")?
            .get("git_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}

fn validate_record(
    record: &Value,
    errors: &mut Vec<String>,
    aliases: &mut HashMap<(String, String, String), String>,
) {
    let id = text(record, "id").unwrap_or_default();
    let kind = text(record, "kind").unwrap_or_default();
    let repository = record
        .get("provenance")
        .and_then(|v| text(v, "repository"))
        .unwrap_or_default();
    let mut id_parts = id.split(':');
    let identity_matches = id_parts.next() == Some("urn")
        && id_parts.next() == Some("research")
        && id_parts.next() == Some(repository)
        && id_parts.next() == Some(kind);
    if !identity_matches {
        errors.push(format!("{id}: identity does not match repository/kind"));
    }
    match revision_digest(record) {
        Ok(expected) if text(record, "revision_id") != Some(expected.as_str()) => errors.push(
            format!("{id}: revision digest does not match semantic content"),
        ),
        Err(error) => errors.push(error.to_string()),
        _ => {}
    }
    if let Some(values) = array(record, "aliases") {
        for alias in values.iter().filter_map(Value::as_str) {
            let key = (repository.to_owned(), kind.to_owned(), alias.to_owned());
            if aliases
                .insert(key.clone(), id.to_owned())
                .is_some_and(|old| old != id)
            {
                errors.push(format!("ambiguous alias: {key:?}"));
            }
        }
    }
    let Some(payload) = record.get("payload") else {
        return;
    };
    if kind == "protocol" {
        if payload
            .get("semantic")
            .and_then(|s| protocol_digest(s).ok())
            .as_deref()
            != payload.get("semantic_digest").and_then(Value::as_str)
        {
            errors.push("protocol semantic digest mismatch".to_owned());
        }
        let historical = record
            .get("provenance")
            .and_then(|v| v.get("historical"))
            .and_then(Value::as_bool)
            == Some(true);
        let freeze = text(payload, "freeze").unwrap_or_default();
        if historical && freeze != "historical-unverified" {
            errors
                .push("historical import cannot fabricate prospective preregistration".to_owned());
        }
        if freeze == "prospective"
            && (payload.get("frozen_at").is_none_or(Value::is_null)
                || payload.get("freeze_evidence").is_none_or(Value::is_null))
        {
            errors.push("prospective freeze requires timestamp and immutable evidence".to_owned());
        }
        if freeze == "historical-unverified"
            && (payload.get("frozen_at").is_some_and(|v| !v.is_null())
                || payload.get("freeze_evidence").is_some_and(|v| !v.is_null()))
        {
            errors.push(
                "unverified historical freeze cannot assert verified freeze evidence".to_owned(),
            );
        }
    }
    if kind == "artifact"
        && text(payload, "availability") == Some("available")
        && payload.get("snapshot_digest").is_none_or(Value::is_null)
    {
        errors.push("available artifact requires immutable snapshot digest".to_owned());
    }
    if kind == "assessment" {
        validate_assessment(record, payload, errors);
    }
}

fn validate_assessment(record: &Value, payload: &Value, errors: &mut Vec<String>) {
    let verdict = text(payload, "verdict").unwrap_or_default();
    if text(payload, "basis") == Some("execution-only")
        && ["supported", "refuted"].contains(&verdict)
    {
        errors.push("execution success/failure is not scientific support/refutation".to_owned());
    }
    if text(payload, "inference") == Some("confirmatory-primary") {
        if !["passed", "not-applicable"].contains(&text(payload, "controls").unwrap_or_default()) {
            errors.push("confirmatory primary inference requires passing controls".to_owned());
        }
        if text(payload, "basis") != Some("scientific-evidence")
            || array(payload, "evidence").is_none_or(<[_]>::is_empty)
        {
            errors.push("confirmatory inference requires scientific evidence".to_owned());
        }
        if text(record, "scope") == Some("unknown") {
            errors.push("confirmatory inference requires explicit scope".to_owned());
        }
        if record_references(record)
            .iter()
            .any(|r| text(r, "status") != Some("resolved"))
        {
            errors.push("pending references cannot be current confirmatory evidence".to_owned());
        }
    }
    if record
        .get("provenance")
        .and_then(|v| v.get("historical"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        if text(payload, "inference") != Some("historical")
            || text(payload, "basis") != Some("legacy-report")
        {
            errors.push("historical assessment must remain a legacy report".to_owned());
        }
        let legacy = text(payload, "legacy_verdict").unwrap_or_default();
        let expected = match legacy {
            "supported" => "supported",
            "refuted" => "refuted",
            "mixed" | "inconclusive" => "inconclusive",
            "conditional" => "conditional",
            "untested" | "conjecture" => "untested",
            _ => "unknown",
        };
        if verdict != expected {
            errors.push("historical verdict was strengthened or changed".to_owned());
        }
        if record
            .get("legacy")
            .and_then(|v| text(v, "status"))
            .is_some_and(|status| status != legacy)
        {
            errors.push("legacy verdict differs from retained source status".to_owned());
        }
    }
}

fn validate_import_report(document: &Value, records: &[&Value], errors: &mut Vec<String>) {
    let inventory = array(document, "inventory").unwrap_or_default();
    let discovered = inventory.len() as u64;
    let mapped = inventory
        .iter()
        .filter(|item| text(item, "disposition") == Some("mapped"))
        .count() as u64;
    let exceptions = inventory
        .iter()
        .filter(|item| text(item, "disposition") == Some("exception"))
        .count() as u64;
    let counts = document.get("counts");
    let unique_keys: HashSet<_> = inventory
        .iter()
        .filter_map(|item| text(item, "key"))
        .collect();
    if counts
        .and_then(|v| v.get("discovered"))
        .and_then(Value::as_u64)
        != Some(discovered)
        || counts.and_then(|v| v.get("mapped")).and_then(Value::as_u64) != Some(mapped)
        || counts
            .and_then(|v| v.get("exceptions"))
            .and_then(Value::as_u64)
            != Some(exceptions)
        || unique_keys.len() != inventory.len()
    {
        errors.push("inventory accounting mismatch or duplicate selector".to_owned());
    }
    let ids: BTreeSet<_> = records.iter().filter_map(|r| text(r, "id")).collect();
    let repository = text(document, "repository").unwrap_or_default();
    let source_revision = document.get("source_revision").and_then(Value::as_str);
    let file_pins: HashSet<_> = array(document, "files")
        .into_iter()
        .flatten()
        .map(|file| (text(file, "path"), text(file, "sha256")))
        .collect();
    if records.iter().any(|record| {
        record
            .get("provenance")
            .and_then(|value| value.get("historical"))
            .and_then(Value::as_bool)
            != Some(true)
    }) {
        errors.push("import candidates must be marked historical".to_owned());
    }
    if records.iter().any(|record| {
        record
            .get("provenance")
            .and_then(|value| text(value, "repository"))
            != Some(repository)
            || record
                .get("provenance")
                .and_then(|value| value.get("git_revision"))
                .and_then(Value::as_str)
                != source_revision
    }) {
        errors.push("candidate provenance differs from import source pin".to_owned());
    }
    if records.iter().any(|record| {
        let provenance = record.get("provenance").unwrap_or(&Value::Null);
        !file_pins.contains(&(text(provenance, "path"), text(provenance, "sha256")))
    }) {
        errors.push("candidate source bytes are absent from file manifest".to_owned());
    }
    let mut accounted = BTreeSet::new();
    for item in inventory {
        if let Some(candidate_ids) = array(item, "candidate_ids") {
            accounted.extend(candidate_ids.iter().filter_map(Value::as_str));
        }
        let exception_items = array(item, "exceptions").unwrap_or_default();
        if text(item, "disposition") == Some("exception") && exception_items.is_empty() {
            errors.push(format!(
                "{}: exception requires a reason",
                text(item, "key").unwrap_or_default()
            ));
        }
        if text(item, "disposition") == Some("mapped")
            && (array(item, "candidate_ids").is_none_or(<[_]>::is_empty)
                || !exception_items.is_empty())
        {
            errors.push(format!(
                "{}: mapped item requires candidates and no exceptions",
                text(item, "key").unwrap_or_default()
            ));
        }
    }
    if accounted != ids {
        errors.push(
            "inventory must account for every candidate and reference only candidates".to_owned(),
        );
    }
    let mut expected_aliases: Vec<_> = records
        .iter()
        .flat_map(|r| {
            array(r, "aliases").into_iter().flatten().filter_map(|a| {
                a.as_str()
                    .map(|alias| (alias, text(r, "id").unwrap_or_default()))
            })
        })
        .collect();
    let mut aliases: Vec<_> = array(document, "aliases")
        .into_iter()
        .flatten()
        .map(|a| {
            (
                text(a, "alias").unwrap_or_default(),
                text(a, "id").unwrap_or_default(),
            )
        })
        .collect();
    expected_aliases.sort_unstable();
    aliases.sort_unstable();
    if aliases != expected_aliases {
        errors.push("alias report mismatch".to_owned());
    }
}

fn validate_manifest(document: &Value, kind: &str, errors: &mut Vec<String>) {
    let manifest = if kind == "manifest" {
        Some(document)
    } else {
        document.get("manifest")
    };
    let Some(manifest) = manifest else { return };
    let repositories = array(manifest, "repositories").unwrap_or_default();
    let ids: HashSet<_> = repositories.iter().filter_map(|p| text(p, "id")).collect();
    if ids.len() != repositories.len() {
        errors.push("manifest repository identity has ambiguous source revisions".to_owned());
    }
    let pins: HashSet<_> = repositories
        .iter()
        .map(|p| (text(p, "id"), p.get("git_revision").and_then(Value::as_str)))
        .collect();
    for reference in array(manifest, "references").unwrap_or_default() {
        if text(reference, "status") == Some("resolved")
            && !pins.contains(&(
                text(reference, "repository"),
                reference.get("source_revision").and_then(Value::as_str),
            ))
        {
            errors.push("resolved reference is not pinned by manifest repositories".to_owned());
        }
    }
}

fn validate_reference(
    reference: &Value,
    known: &HashMap<RecordKey, &Value>,
    records: &[&Value],
    errors: &mut Vec<String>,
) {
    let id = text(reference, "id").unwrap_or_default();
    let repository = text(reference, "repository").unwrap_or_default();
    if id.split(':').nth(2) != Some(repository) {
        errors.push("reference identity does not match repository".to_owned());
    }
    if text(reference, "status") != Some("resolved") {
        return;
    }
    let key = (
        id.to_owned(),
        text(reference, "revision_id")
            .unwrap_or_default()
            .to_owned(),
        reference
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    );
    let target = known.get(&key).copied();
    let exact = target.is_some_and(|target| {
        text(
            target.get("provenance").unwrap_or(&Value::Null),
            "repository",
        ) == Some(repository)
            && reference
                .get("source_revision")
                .and_then(Value::as_str)
                .is_some()
            && target
                .get("provenance")
                .and_then(|v| v.get("working_tree"))
                .and_then(Value::as_bool)
                == Some(false)
    });
    if !exact {
        errors.push(format!(
            "{id}: resolved reference lacks exact validated target/source pin"
        ));
        return;
    }
    if target
        .and_then(|v| v.get("payload"))
        .and_then(|v| text(v, "controls"))
        == Some("failed")
    {
        for record in records {
            if text(record, "kind") == Some("assessment")
                && record.get("payload").and_then(|v| text(v, "inference"))
                    == Some("confirmatory-primary")
            {
                errors.push(
                    "failed-control experiment cannot support primary confirmation".to_owned(),
                );
            }
        }
    }
    if let Some(assessment) = records.iter().find(|record| {
        text(record, "kind") == Some("assessment")
            && record.get("payload").and_then(|v| text(v, "inference"))
                == Some("confirmatory-primary")
    }) {
        let payload = assessment.get("payload").unwrap_or(&Value::Null);
        let claim = payload
            .get("claim")
            .and_then(reference_key)
            .and_then(|claim_key| known.get(&claim_key).copied());
        if claim
            .and_then(|value| value.get("payload"))
            .and_then(|value| text(value, "domain"))
            == Some("nature")
        {
            let model_scopes = [
                "derivation",
                "simulation-under-assumptions",
                "synthetic-calibration",
            ];
            if model_scopes.contains(&text(assessment, "scope").unwrap_or_default())
                || array(payload, "evidence")
                    .into_iter()
                    .flatten()
                    .filter_map(reference_key)
                    .filter_map(|evidence_key| known.get(&evidence_key).copied())
                    .any(|evidence| {
                        model_scopes.contains(&text(evidence, "scope").unwrap_or_default())
                    })
            {
                errors.push(
                    "model or synthetic evidence cannot confirm a claim about nature".to_owned(),
                );
            }
        }
    }
}

/// Every typed scientific edge a record carries, including native `supersedes` links.
pub fn record_references(record: &Value) -> Vec<&Value> {
    let mut refs: Vec<&Value> = array(record, "references").into_iter().flatten().collect();
    if let Some(authorship) = record.get("authorship") {
        refs.extend(array(authorship, "supersedes").into_iter().flatten());
    }
    let Some(payload) = record.get("payload") else {
        return refs;
    };
    match text(record, "kind") {
        Some("assessment") => {
            if let Some(claim) = payload.get("claim") {
                refs.push(claim);
            }
            refs.extend(array(payload, "evidence").into_iter().flatten());
        }
        Some("experiment") => {
            if let Some(protocol) = payload.get("protocol").filter(|v| !v.is_null()) {
                refs.push(protocol);
            }
            refs.extend(array(payload, "result_artifacts").into_iter().flatten());
            refs.extend(array(payload, "inputs").into_iter().flatten());
            if let Some(start) = payload.get("start").filter(|v| !v.is_null()) {
                refs.push(start);
            }
        }
        Some("protocol") => {
            if let Some(evidence) = payload.get("freeze_evidence").filter(|v| !v.is_null()) {
                refs.push(evidence);
            }
            if record.get("schema_version").and_then(Value::as_u64) == Some(2)
                && let Some(semantic) = payload.get("semantic")
            {
                refs.extend(array(semantic, "claims").into_iter().flatten());
                refs.extend(array(semantic, "inputs").into_iter().flatten());
            }
        }
        _ => {}
    }
    refs
}

pub fn reconcile(manifest: &Value, records: &[Value]) -> Result<Value, ContractError> {
    let schema = schema_errors(manifest);
    if !schema.is_empty() {
        return Err(ContractError::Schema(schema.join("; ")));
    }
    if text(manifest, "kind") != Some("manifest") {
        return Err(ContractError::Expected("a manifest"));
    }
    let mut result = manifest.clone();
    let mut valid = HashMap::new();
    let mut duplicates = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        let context: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, value)| value.clone())
            .collect();
        if validate(record, &context).is_empty()
            && let Some(key) = full_pin(record)
            && valid.insert(key.clone(), record).is_some()
        {
            duplicates.insert(key);
        }
    }
    let pins: HashSet<_> = array(manifest, "repositories")
        .unwrap_or_default()
        .iter()
        .map(|p| {
            (
                text(p, "id").map(str::to_owned),
                p.get("git_revision")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )
        })
        .collect();
    if let Some(references) = result.get_mut("references").and_then(Value::as_array_mut) {
        for reference in references {
            let key = reference_pin(reference);
            let resolved = key
                .as_ref()
                .and_then(|key| valid.get(key).map(|record| (key, *record)))
                .is_some_and(|(key, record)| {
                    !duplicates.contains(key)
                        && key.3.is_some()
                        && record
                            .get("provenance")
                            .and_then(|v| v.get("working_tree"))
                            .and_then(Value::as_bool)
                            == Some(false)
                        && pins.contains(&(Some(key.0.clone()), key.3.clone()))
                });
            reference["status"] =
                Value::String(if resolved { "resolved" } else { "pending" }.to_owned());
        }
    }
    Ok(result)
}

type FullPin = (String, String, String, Option<String>);
fn full_pin(record: &Value) -> Option<FullPin> {
    Some((
        text(record.get("provenance")?, "repository")?.to_owned(),
        text(record, "id")?.to_owned(),
        text(record, "revision_id")?.to_owned(),
        record
            .get("provenance")?
            .get("git_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}
fn reference_pin(reference: &Value) -> Option<FullPin> {
    Some((
        text(reference, "repository")?.to_owned(),
        text(reference, "id")?.to_owned(),
        text(reference, "revision_id")?.to_owned(),
        reference
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}
fn reference_key(reference: &Value) -> Option<RecordKey> {
    Some((
        text(reference, "id")?.to_owned(),
        text(reference, "revision_id")?.to_owned(),
        reference
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn array<'a>(value: &'a Value, key: &str) -> Option<&'a [Value]> {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATION_REPORT: &[u8] = include_bytes!("../../../examples/migration-report.json");

    fn value(source: &str) -> Value {
        parse_json(source.as_bytes()).expect("valid test JSON")
    }

    #[test]
    fn migration_candidate_digests_match_python_oracle() {
        let report = parse_json(MIGRATION_REPORT).expect("fixture parses");
        let candidates = array(&report, "candidates").expect("candidates");
        for candidate in candidates {
            assert_eq!(
                revision_digest(candidate).expect("digest"),
                text(candidate, "revision_id").expect("revision")
            );
            if text(candidate, "kind") == Some("protocol") {
                let payload = candidate.get("payload").expect("payload");
                assert_eq!(
                    protocol_digest(payload.get("semantic").expect("semantic")).expect("digest"),
                    text(payload, "semantic_digest").expect("semantic digest")
                );
            }
        }
        assert!(
            validate(&report, &[]).is_empty(),
            "{:?}",
            validate(&report, &[])
        );
    }

    #[test]
    fn nested_legacy_non_ascii_and_large_integer_match_cpython() {
        let record = value(
            r#"{"schema_version":1,"kind":"claim","id":"urn:research:test:claim:caf%C3%A9","revision_id":"ignored","aliases":["café"],"activity":"unknown","scope":"unknown","provenance":{},"limitations":[],"missingness":[],"legacy":{"z":{"β":"雪","a":9007199254740993}},"references":[],"presentation":{"ignored":true},"payload":{"statement":"naïve","role":"claim","domain":"model"}}"#,
        );
        assert_eq!(
            revision_digest(&record).expect("digest"),
            "sha256:1e4bae10937b86304b66593bd37dd0d248106df341d55299999a5e77054c6dfd"
        );
    }

    #[test]
    fn canonical_numbers_match_cpython_boundaries() {
        let numbers = value("[1.0,1e15,1e16,1e-4,1e-5,-0.0]");
        assert_eq!(
            String::from_utf8(canonical_json(&numbers).expect("canonical")).expect("utf8"),
            "[1.0,1000000000000000.0,1e+16,0.0001,1e-05,-0.0]"
        );
    }

    #[test]
    fn duplicate_keys_are_rejected_at_any_depth() {
        let error =
            parse_json(br#"{"outer":{"same":1,"same":2}}"#).expect_err("duplicate rejected");
        assert!(error.to_string().contains("duplicate object key: same"));
    }
}
