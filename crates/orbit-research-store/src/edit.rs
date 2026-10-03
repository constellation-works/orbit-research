//! Record edit policy and whole-corpus checks shared by both writer modes.
//! Nothing here reads Git or the clock: callers pass records, bytes and dates.
use crate::{
    Error, Result, record,
    validation::{Contract, validate_records},
};
use orbit_research_common::Record;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// Requested changes to an existing record. `None` keeps the current value.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Edit {
    pub title: Option<String>,
    /// Complete Markdown body after the frontmatter.
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    /// Research only: hypotheses this result tests.
    pub tests: Option<Vec<String>>,
    /// Research only: operational provenance.
    pub orbit: Option<OrbitLink>,
    /// Research only: replacement `data/manifest.json` contents.
    pub manifest: Option<Value>,
}

impl Edit {
    /// True when no field asks for a change.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.body.is_none()
            && self.tags.is_none()
            && self.status.is_none()
            && self.tests.is_none()
            && self.orbit.is_none()
            && self.manifest.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OrbitLink {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// One explicit verdict on a hypothesis revision. The writer never infers it.
#[derive(Debug, Clone, Serialize)]
pub struct Assessment {
    pub research: String,
    pub revision: u64,
    pub verdict: String,
    pub strength: String,
    pub note: Option<String>,
}

/// Render `record` with `edit` applied. Identity, path, lineage and assessments
/// stay frozen. A hypothesis statement (title or body) change bumps `revision`
/// and reopens its status, because earlier verdicts stay on their revision.
pub(crate) fn revise(
    contract: &Contract,
    record: &Record,
    edit: &Edit,
    today: &str,
) -> Result<String> {
    if record.kind != "R"
        && (edit.tests.is_some() || edit.orbit.is_some() || edit.manifest.is_some())
    {
        return Err(Error::InvalidInput(
            "tests, orbit and manifest apply only to research records".into(),
        ));
    }
    let mut metadata = record.metadata.clone();
    let old_title = metadata["title"].as_str().unwrap_or_default().to_owned();
    let title = match &edit.title {
        Some(title) => checked_title(title)?.to_owned(),
        None => old_title.clone(),
    };
    let body_changed = edit
        .body
        .as_deref()
        .is_some_and(|body| body.trim() != record.body.trim());
    if title != old_title {
        let slug = frozen_slug(&record.path)?;
        if metadata["slug"].is_null() && record::kebab(&title) != slug {
            metadata["slug"] = json!(slug);
        }
        metadata["title"] = json!(title);
    }
    if record.kind == "H" && (title != old_title || body_changed) {
        let revision = metadata["revision"]
            .as_u64()
            .ok_or_else(|| Error::Invalid(format!("{} has no revision", record.id)))?;
        metadata["revision"] = json!(revision + 1);
        if metadata["status"] != "dropped" {
            metadata["status"] = json!("open");
        }
    }
    if let Some(tags) = &edit.tags {
        metadata["tags"] = json!(tags);
    }
    if let Some(status) = &edit.status {
        metadata["status"] = json!(status);
    }
    if let Some(tests) = &edit.tests {
        metadata["tests"] = json!(tests);
    }
    if let Some(orbit) = &edit.orbit {
        metadata["orbit"] = serde_json::to_value(orbit)?;
    }
    metadata["updated"] = json!(today);
    contract.validate(&metadata, &record.path)?;
    let raw = match &edit.body {
        Some(body) => format!("\n{body}\n"),
        None => record.body.clone(),
    };
    let raw = if title == old_title {
        raw
    } else {
        retitle_heading(&raw, &record.id, &old_title, &title)
    };
    record::render(&metadata, &raw)
}

/// Follow a title change in the body's heading. Only the first non-blank line,
/// and only in the scaffolded `# <ID> — <old title>` form, is rewritten; any
/// other heading is the author's own words and stays as written.
fn retitle_heading(raw: &str, id: &str, old_title: &str, title: &str) -> String {
    let old_heading = format!("# {id} — {old_title}");
    let mut offset = 0;
    for line in raw.split_inclusive('\n') {
        let text = line.trim_end_matches(['\n', '\r']);
        if text.trim().is_empty() {
            offset += line.len();
            continue;
        }
        if text == old_heading {
            let end = offset + line.len();
            return format!(
                "{}# {id} — {title}{}{}",
                &raw[..offset],
                &line[text.len()..],
                &raw[end..]
            );
        }
        break;
    }
    raw.to_owned()
}

/// Append one assessment to a hypothesis. Earlier entries are copied unchanged
/// and in order. Status follows the owner schema's `verdict_status` only for the
/// current revision, and a `dropped` hypothesis stays dropped.
pub(crate) fn assess(
    contract: &Contract,
    records: &[Record],
    record: &Record,
    assessment: &Assessment,
    today: &str,
) -> Result<String> {
    if record.kind != "H" {
        return Err(Error::Invalid("Only hypotheses carry assessments".into()));
    }
    let mut metadata = record.metadata.clone();
    let current = metadata["revision"]
        .as_u64()
        .ok_or_else(|| Error::Invalid(format!("{} has no revision", record.id)))?;
    if assessment.revision == 0 || assessment.revision > current {
        return Err(Error::Invalid(format!(
            "{} has no revision {}; its current revision is {current}",
            record.id, assessment.revision
        )));
    }
    match records.iter().find(|r| r.id == assessment.research) {
        Some(research) if research.kind == "R" => (),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "Assessments cite research records; {} is a {} record",
                other.id, other.kind
            )));
        }
        None => {
            return Err(Error::NotFound(format!(
                "Unknown research record id: {}",
                assessment.research
            )));
        }
    }
    let mut entry = json!({
        "date": today,
        "research": assessment.research,
        "revision": assessment.revision,
        "verdict": assessment.verdict,
        "strength": assessment.strength,
    });
    if let Some(note) = assessment.note.as_deref().map(str::trim)
        && !note.is_empty()
    {
        entry["note"] = json!(note);
    }
    let mut entries = metadata["assessments"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    entries.push(entry);
    metadata["assessments"] = Value::Array(entries);
    if assessment.revision == current
        && metadata["status"] != "dropped"
        && let Some(status) =
            contract.schema["x-observatory"]["verdict_status"][&assessment.verdict].as_str()
    {
        metadata["status"] = json!(status);
    }
    metadata["updated"] = json!(today);
    contract.validate(&metadata, &record.path)?;
    record::render(&metadata, &record.body)
}

/// Canonical `data/manifest.json` bytes, checked against the owner schema.
pub(crate) fn manifest_text(contract: &Contract, manifest: &Value) -> Result<String> {
    check_manifest(contract, manifest)?;
    Ok(format!("{}\n", serde_json::to_string_pretty(manifest)?))
}

/// Check a `data/manifest.json` value against the owner schema's
/// `data_manifest` definition when it exports one.
pub(crate) fn check_manifest(contract: &Contract, manifest: &Value) -> Result<()> {
    if contract.schema["$defs"]["data_manifest"].is_object() {
        let mut schema = contract.schema.clone();
        if let Some(root) = schema.as_object_mut() {
            root.remove("oneOf");
            root.insert("$ref".into(), json!("#/$defs/data_manifest"));
        }
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .map_err(|e| Error::Invalid(format!("Invalid owner schema: {e}")))?;
        if let Err(errors) = validator.validate(manifest) {
            return Err(Error::InvalidInput(format!(
                "data/manifest.json: {}",
                errors.map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
            )));
        }
    } else if !manifest["inputs"].is_array() {
        return Err(Error::InvalidInput(
            "data/manifest.json must list its inputs".into(),
        ));
    }
    Ok(())
}

/// Check proposed record bytes against the owner schema and the rest of the
/// corpus (references, numbering, lineage) before anything is written.
pub(crate) fn check_records(
    contract: &Contract,
    records: &[Record],
    path: &str,
    text: &str,
) -> Result<()> {
    let (metadata, body) = record::parse(text)?;
    contract.validate(&metadata, path)?;
    let mut all: BTreeMap<String, Record> =
        records.iter().map(|r| (r.id.clone(), r.clone())).collect();
    let changed = all
        .values_mut()
        .find(|r| r.path == path)
        .ok_or_else(|| Error::Invalid(format!("{path} is not a corpus record")))?;
    if metadata["id"] != changed.id.as_str() {
        return Err(Error::Invalid(format!("Record ID/path mismatch: {path}")));
    }
    changed.metadata = metadata;
    changed.body = body;
    validate_records(&all)
}

pub(crate) fn checked_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::Invalid("Title is required".into()));
    }
    Ok(title)
}

/// The slug frozen in a record's path: its file stem, or its directory for R.
fn frozen_slug(path: &str) -> Result<&str> {
    let path = Path::new(path);
    let name = if path.file_name().is_some_and(|name| name == "README.md") {
        path.parent().and_then(Path::file_name)
    } else {
        path.file_stem()
    };
    name.and_then(|name| name.to_str())
        .and_then(|name| name.split_once('-'))
        .map(|(_, slug)| slug)
        .ok_or_else(|| Error::Invalid("Missing frozen slug".into()))
}
