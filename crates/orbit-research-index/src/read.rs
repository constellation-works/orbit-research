//! Read the published projection and derive an exact reproducibility trace from it. Ports
//! `index.py::read_index` / `index.py::trace`.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::{IndexError, Result, require};

/// The published projection, digest-checked on every read: a corrupted or hand-edited
/// database file is refused rather than silently trusted.
pub fn read_index(database: &Path) -> Result<Value> {
    let path = database
        .canonicalize()
        .map_err(|error| IndexError::Invalid(format!("{}: {error}", database.display())))?;
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let body: String = conn.query_row("SELECT body FROM projection WHERE id = 1", [], |row| row.get(0))?;
    let mut result = orbit_research_contract::parse_json(body.as_bytes())?;
    let digest = result
        .as_object_mut()
        .and_then(|object| object.remove("content_digest"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| IndexError::Invalid("invalid projection body".into()))?;
    let recomputed = crate::digest_bytes(&orbit_research_contract::canonical_json(&result)?);
    require(
        digest == recomputed,
        "projection content digest mismatch; rebuild from owner files",
    )?;
    result["content_digest"] = Value::String(digest);
    Ok(result)
}

/// The selected snapshot, every assessment of that exact claim/source pin, and their
/// transitive dependencies, including unresolved edges.
pub fn trace(database: &Path, record_key: &str) -> Result<Value> {
    let projection = read_index(database)?;
    let records = projection
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let nodes: HashMap<&str, &Value> = records
        .iter()
        .filter_map(|node| node.get("key").and_then(Value::as_str).map(|key| (key, node)))
        .collect();
    require(
        nodes.contains_key(record_key),
        "trace requires an exact indexed snapshot key",
    )?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue = vec![record_key.to_owned()];
    while let Some(key) = queue.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(node) = nodes.get(key.as_str()) else {
            continue;
        };
        for assessment in node.get("assessments").and_then(Value::as_array).into_iter().flatten() {
            if let Some(assessment) = assessment.as_str() {
                queue.push(assessment.to_owned());
            }
        }
        for edge in node.get("edges").and_then(Value::as_array).into_iter().flatten() {
            if let Some(target) = edge.get("target").and_then(Value::as_str) {
                queue.push(target.to_owned());
            }
        }
    }
    let traced: Vec<Value> = seen
        .iter()
        .filter_map(|key| nodes.get(key.as_str()).map(|node| (*node).clone()))
        .collect();
    Ok(json!({
        "root": record_key,
        "records": traced,
        "content_digest": projection.get("content_digest").cloned().unwrap_or(Value::Null),
    }))
}
