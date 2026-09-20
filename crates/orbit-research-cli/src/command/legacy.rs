//! Compatibility command handlers retained from the original JSON CLI.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use orbit_research_core::legacy_contract::{parse_json, reconcile, validate};
use orbit_research_core::legacy_import::{ADAPTERS, import_source, write_report};
use orbit_research_core::legacy_owner::{reference, write_json_new};
use serde_json::{Value, json};

use crate::output::Invalid;
use crate::parse::{Authoring, OwnerArgs};

pub(crate) fn resource() -> (Value, u8) {
    (
        json!({"version":1,"skill":orbit_research_core::RESEARCH_NATIVE_SKILL}),
        0,
    )
}

pub(crate) fn validate_file(input: &Path, targets: &[PathBuf]) -> Result<(Value, u8), Invalid> {
    let document = read_json(input)?;
    let targets = read_targets(targets)?;
    let errors = validate(&document, &targets);
    Ok((
        json!({"valid":errors.is_empty(),"errors":errors}),
        if errors.is_empty() { 0 } else { 1 },
    ))
}

pub(crate) fn author(operation: &str, args: &Authoring) -> Result<(Value, u8), Invalid> {
    let owner = args.owner.open()?;
    let request = read_json(&args.request)?;
    Ok((
        owner
            .apply(operation, &request)
            .map_err(|error| error.to_string())?,
        0,
    ))
}

pub(crate) fn owner_heads(owner: OwnerArgs, id: &str) -> Result<(Value, u8), Invalid> {
    let owner = owner.open()?;
    Ok((
        json!({"heads":owner.heads(id).map_err(|error| error.to_string())?}),
        0,
    ))
}

pub(crate) fn owner_ref(
    owner: OwnerArgs,
    id: &str,
    revision: &str,
    source_revision: &str,
) -> Result<(Value, u8), Invalid> {
    let owner = owner.open()?;
    let pinned = owner
        .pin(id, revision, source_revision)
        .map_err(|error| error.to_string())?;
    Ok((reference(&pinned, "resolved"), 0))
}

pub(crate) fn owner_trace(
    owner: OwnerArgs,
    id: &str,
    revision: &str,
) -> Result<(Value, u8), Invalid> {
    let owner = owner.open()?;
    Ok((
        owner
            .trace(id, revision)
            .map_err(|error| error.to_string())?,
        0,
    ))
}

pub(crate) fn owner_export(
    owner: OwnerArgs,
    source_revision: &str,
    output: &Path,
) -> Result<(Value, u8), Invalid> {
    let owner = owner.open()?;
    let destination = resolved_destination(output)?;
    if destination.starts_with(
        owner
            .directory()
            .canonicalize()
            .unwrap_or_else(|_| owner.directory().to_path_buf()),
    ) {
        return Err("export cannot write inside canonical records"
            .to_owned()
            .into());
    }
    let bundle = owner
        .export(source_revision)
        .map_err(|error| error.to_string())?;
    write_json_new(&bundle, output).map_err(|error| error.to_string())?;
    Ok((
        json!({"output":output.to_string_lossy(),"records":bundle["records"].as_array().map_or(0,Vec::len),"manifests":bundle["manifests"].as_array().map_or(0,Vec::len)}),
        0,
    ))
}

pub(crate) fn reconcile_file(
    input: &Path,
    targets: &[PathBuf],
    output: &Path,
) -> Result<(Value, u8), Invalid> {
    let manifest = read_json(input)?;
    let targets = read_targets(targets)?;
    let result = reconcile(&manifest, &targets).map_err(|error| error.to_string())?;
    let errors = validate(&result, &targets);
    if !errors.is_empty() {
        return Err(errors.join("; ").into());
    }
    write_new(output, &result)?;
    Ok((json!({"output":output.to_string_lossy()}), 0))
}

pub(crate) fn index(config: &Path, database: &Path) -> Result<(Value, u8), Invalid> {
    let outcome = orbit_research_core::legacy_index::rebuild(config, database)?;
    Ok((
        json!({"database":outcome.database.to_string_lossy(),"records":outcome.records,"content_digest":outcome.content_digest,"pending":outcome.pending}),
        0,
    ))
}
pub(crate) fn index_trace(database: &Path, key: &str) -> Result<(Value, u8), Invalid> {
    Ok((orbit_research_core::legacy_index::trace(database, key)?, 0))
}
pub(crate) fn browse_export(
    config: &Path,
    database: &Path,
    output: &Path,
) -> Result<(Value, u8), Invalid> {
    let outcome = orbit_research_core::legacy_index::export_browser(database, output, config)?;
    Ok((
        json!({"output":outcome.output.to_string_lossy(),"records":outcome.records,"content_digest":outcome.content_digest}),
        0,
    ))
}

pub(crate) fn import_file(
    adapter: &str,
    source_root: &Path,
    repository: &str,
    expect_revision: Option<&str>,
    select: &[String],
    output: Option<&Path>,
) -> Result<(Value, u8), Invalid> {
    if !ADAPTERS.contains(&adapter) {
        return Err(format!("adapter must be one of: {}", ADAPTERS.join(", ")).into());
    }
    let selected = (!select.is_empty()).then_some(select);
    let report = import_source(source_root, adapter, repository, selected, expect_revision)
        .map_err(|error| error.to_string())?;
    let errors = validate(&report, &[]);
    if !errors.is_empty() {
        return Err(format!(
            "candidate validation failed: {}",
            errors
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        )
        .into());
    }
    if let Some(output) = output {
        write_report(&report, output, &[source_root.to_path_buf()])
            .map_err(|error| error.to_string())?;
        Ok((
            json!({"output":output.to_string_lossy(),"counts":report["counts"],"source_unchanged":true}),
            0,
        ))
    } else {
        Ok((report, 0))
    }
}

fn read_json(path: &Path) -> Result<Value, Invalid> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_json(&bytes).map_err(|error| format!("{}: {error}", path.display()).into())
}
fn read_targets(paths: &[PathBuf]) -> Result<Vec<Value>, Invalid> {
    paths.iter().map(|path| read_json(path)).collect()
}
fn resolved_destination(output: &Path) -> Result<PathBuf, Invalid> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = parent.map_or_else(
        || std::env::current_dir().map_err(|error| error.to_string()),
        |parent| {
            parent
                .canonicalize()
                .map_err(|error| format!("{}: {error}", parent.display()))
        },
    )?;
    Ok(output
        .file_name()
        .map_or(directory.clone(), |name| directory.join(name)))
}
fn write_new(path: &Path, value: &Value) -> Result<(), Invalid> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, value).map_err(|error| error.to_string())?;
    Ok(file.write_all(b"\n").map_err(|error| error.to_string())?)
}
