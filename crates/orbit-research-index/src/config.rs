//! Operator-authored index configuration: explicit checkouts and document paths, no
//! recursive whole-machine discovery. Ports `index.py::load_config` / `guard_output`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orbit_research_contract::RepositoryId;
use orbit_research_owner::{Checkout, SubprocessGit};
use serde_json::Value;

use crate::{IndexError, Result, require, text};

const CONFIG_FIELDS: &[&str] = &[
    "schema_version",
    "checkouts",
    "documents",
    "media",
    "checkout_urls",
];

/// A loaded, validated index configuration: the raw document, opened checkouts and the
/// exact, sorted list of owner document paths it selects.
pub struct IndexConfig {
    pub raw: Value,
    pub roots: BTreeMap<String, Checkout>,
    pub paths: Vec<PathBuf>,
}

pub fn load_config(path: &Path) -> Result<IndexConfig> {
    let path = path
        .canonicalize()
        .map_err(|error| IndexError::Invalid(format!("{}: {error}", path.display())))?;
    let config = orbit_research_contract::parse_json(&fs::read(&path)?)?;
    let object = config.as_object().ok_or_else(config_shape_error)?;
    let unknown = object
        .keys()
        .any(|key| !CONFIG_FIELDS.contains(&key.as_str()));
    require(
        config.get("schema_version").and_then(Value::as_u64) == Some(1)
            && !unknown
            && config.get("checkouts").is_some_and(Value::is_object)
            && config.get("documents").is_some_and(Value::is_array),
        "index config requires schema_version 1, checkouts object and documents array",
    )?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut roots = BTreeMap::new();
    for (repository, root) in config["checkouts"].as_object().expect("checked above") {
        require(
            repository.parse::<RepositoryId>().is_ok() && root.is_string(),
            "invalid checkout namespace",
        )?;
        let checkout = Checkout::open(
            &parent.join(root.as_str().unwrap_or_default()),
            Arc::new(SubprocessGit),
        )?;
        roots.insert(repository.clone(), checkout);
    }
    let documents = config["documents"].as_array().expect("checked above");
    require(
        documents
            .iter()
            .all(|value| value.as_str().is_some_and(|value| !value.is_empty())),
        "documents must be non-empty path strings",
    )?;
    require(
        config.get("checkout_urls").is_none_or(Value::is_object),
        "checkout_urls must be an object",
    )?;
    require(
        config.get("media").is_none_or(Value::is_array),
        "media must be an array",
    )?;
    for item in config
        .get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        require(
            item.get("record").is_some_and(valid_reference_shape),
            "media entries require an exact record reference object",
        )?;
        require(
            item.as_object().is_some_and(|object| {
                object
                    .iter()
                    .filter(|(key, _)| key.as_str() != "record")
                    .all(|(_, value)| value.is_string())
            }),
            "media fields other than record must be strings",
        )?;
    }
    let mut paths = Vec::new();
    for relative in documents {
        let relative = relative.as_str().unwrap_or_default();
        let candidate = parent.join(relative);
        require(
            !has_symlink_component(&candidate),
            format!("document symlink is forbidden: {}", candidate.display()),
        )?;
        if candidate.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(&candidate)?
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|suffix| suffix == "json"))
                .collect();
            entries.sort();
            paths.extend(entries);
        } else {
            paths.push(candidate);
        }
    }
    require(!paths.is_empty(), "no owner documents selected")?;
    paths.sort();
    paths.dedup();
    Ok(IndexConfig {
        raw: config,
        roots,
        paths,
    })
}

/// Best-effort shape check for one media/manifest reference: required fields present with
/// plausible types. Exact target existence is derived independently by `build_projection`,
/// which never trusts a reference's own `status` field.
pub(crate) fn valid_reference_shape(record: &Value) -> bool {
    record.is_object()
        && text(record, "repository").is_some()
        && text(record, "id").is_some()
        && text(record, "revision_id").is_some()
        && matches!(
            record.get("source_revision"),
            None | Some(Value::Null) | Some(Value::String(_))
        )
        && matches!(text(record, "status"), Some("pending") | Some("resolved"))
}

fn config_shape_error() -> IndexError {
    IndexError::Invalid(
        "index config requires schema_version 1, checkouts object and documents array".into(),
    )
}

fn has_symlink_component(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    })
}

/// Resolve a path that may not exist yet: canonicalize the longest existing ancestor, then
/// append the remaining components literally. Mirrors Python `Path.resolve()` on a
/// not-yet-created destination.
fn resolve_loose(path: &Path) -> Result<PathBuf> {
    let mut existing = path;
    let mut remainder = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            break;
        };
        remainder.push(name.to_owned());
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|error| IndexError::Invalid(format!("{}: {error}", existing.display())))?;
    for part in remainder.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

/// The projection output must be new territory: outside every owner checkout and never
/// overlapping a supplied input path. Returns the resolved, canonical output path.
pub fn guard_output(
    output: &Path,
    roots: &BTreeMap<String, Checkout>,
    inputs: &[PathBuf],
) -> Result<PathBuf> {
    require(!has_symlink_component(output), "output may not use symlinks")?;
    let resolved = resolve_loose(output)?;
    require(
        !roots.values().any(|root| resolved.starts_with(root.root())),
        "projection output must be outside every owner checkout",
    )?;
    for input in inputs {
        let resolved_input = input.canonicalize().unwrap_or_else(|_| input.clone());
        require(
            resolved != resolved_input && !resolved_input.starts_with(&resolved),
            "output overlaps index input",
        )?;
    }
    Ok(resolved)
}
