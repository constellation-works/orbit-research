//! Read-only owner adapters: principia, parallax, orrery, astrolabe.
//!
//! Ports `src/orbit_research/importers.py`. Only depends on
//! `orbit-research-contract`; never `orbit-research-owner`, so `Owner::apply`
//! is not even nameable from here. Every public entry point is a dry run:
//! `import_source` never mutates the source tree, and `write_report` refuses
//! to write inside it.

mod git;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use regex::Regex;
use rusqlite::types::ValueRef;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub use orbit_research_contract::RepositoryId;

pub const ADAPTERS: &[&str] = &["principia", "parallax", "orrery", "astrolabe"];

const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("{0}")]
    Value(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Contract(#[from] orbit_research_contract::ContractError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn value_error(message: impl Into<String>) -> ImportError {
    ImportError::Value(message.into())
}

pub fn patterns_for(adapter: &str) -> Option<&'static [&'static str]> {
    match adapter {
        "principia" => Some(&[
            "theory/**/claims.json",
            "theory/**/*.md",
            "gates/*.json",
            "studies/*preregistration*.md",
            "ledger.md",
        ]),
        "parallax" => Some(&[
            "docs/*.md",
            "docs/research/**/*.md",
            "data/**/*.db",
            "data/**/*.sqlite",
            "data/**/*.sqlite3",
            "artifacts/**/*.db",
            "artifacts/**/*.sqlite",
            "artifacts/**/*.sqlite3",
        ]),
        "orrery" => Some(&["lab/sims/**/*.json"]),
        "astrolabe" => Some(&["data/processed/**/*.json"]),
        _ => None,
    }
}

const ACTIVITY: &[&str] = &["active", "paused", "retired", "resolved"];

fn is_activity(value: &str) -> bool {
    ACTIVITY.contains(&value)
}

fn verdict_for(status: Option<&str>) -> &'static str {
    match status {
        Some("supported") => "supported",
        Some("refuted") => "refuted",
        Some("mixed") => "inconclusive",
        Some("inconclusive") => "inconclusive",
        Some("conditional") => "conditional",
        Some("untested") => "untested",
        Some("conjecture") => "untested",
        _ => "unknown",
    }
}

/// Python truthiness for an already-parsed JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn file_digest(path: &Path) -> Result<String, ImportError> {
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

/// Match CPython `urllib.parse.quote(str(value), safe="")`: percent-encode
/// every byte except unreserved ASCII, using upper-case hex.
fn quote_safe_empty(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let c = *byte as char;
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~') {
            out.push(c);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Resolve as many leading path components as exist (following symlinks),
/// then append the remaining components lexically. Mirrors Python's
/// `Path.resolve()`, which does not require the full path to exist.
fn resolve_best_effort(path: &Path) -> PathBuf {
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(canonical) = probe.canonicalize() {
            let mut result = canonical;
            for part in suffix.into_iter().rev() {
                result.push(part);
            }
            return result;
        }
        match (probe.file_name().map(|n| n.to_os_string()), probe.parent()) {
            (Some(name), Some(parent)) => {
                suffix.push(name);
                probe = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn contained(root: &Path, path: &Path) -> bool {
    resolve_best_effort(path).starts_with(root)
}

fn has_forbidden_component(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git") | Some(".orbit")
        )
    })
}

fn discover(
    root: &Path,
    adapter: &str,
    selected: Option<&[String]>,
) -> Result<(Vec<PathBuf>, Vec<String>), ImportError> {
    if let Some(selected) = selected {
        if !selected.is_empty() {
            let mut paths = Vec::new();
            for rel in selected {
                let rel_path = Path::new(rel);
                let path = root.join(rel_path);
                if rel_path.is_absolute() || !contained(root, &path) || has_forbidden_component(rel_path) {
                    return Err(value_error(format!(
                        "selected input must remain in scientific source root: {rel}"
                    )));
                }
                if !path.is_file() {
                    return Err(value_error(format!("selected input is not a file: {rel}")));
                }
                paths.push(path);
            }
            let unique: BTreeSet<PathBuf> = paths.into_iter().collect();
            return Ok((unique.into_iter().collect(), selected.to_vec()));
        }
    }
    let patterns = patterns_for(adapter).ok_or_else(|| value_error("unsupported adapter"))?;
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    for pattern in patterns {
        for path in glob_files(root, pattern) {
            if path.is_file()
                && let Ok(rel) = path.strip_prefix(root)
                && !has_forbidden_component(rel)
            {
                set.insert(path);
            }
        }
    }
    Ok((
        set.into_iter().collect(),
        patterns.iter().map(|s| s.to_string()).collect(),
    ))
}

fn glob_files(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let components: Vec<&str> = pattern.split('/').collect();
    let mut results = Vec::new();
    walk_glob(root, &components, &mut results);
    results
}

fn walk_glob(dir: &Path, components: &[&str], results: &mut Vec<PathBuf>) {
    let Some((first, rest)) = components.split_first() else {
        return;
    };
    if *first == "**" {
        walk_glob(dir, rest, results);
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| !has_forbidden_component(p.file_name().map(Path::new).unwrap_or(Path::new(""))))
            .collect();
        subdirs.sort();
        for sub in subdirs {
            walk_glob(&sub, components, results);
        }
        return;
    }
    if rest.is_empty() {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut matches: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| wildcard_match(first, name))
            })
            .collect();
        matches.sort();
        results.extend(matches);
        return;
    }
    let next = dir.join(first);
    if next.is_dir() {
        walk_glob(&next, rest, results);
    }
}

fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn helper(pattern: &[u8], name: &[u8]) -> bool {
        match pattern.first() {
            None => name.is_empty(),
            Some(b'*') => {
                for i in 0..=name.len() {
                    if helper(&pattern[1..], &name[i..]) {
                        return true;
                    }
                }
                false
            }
            Some(&c) => !name.is_empty() && name[0] == c && helper(&pattern[1..], &name[1..]),
        }
    }
    helper(pattern.as_bytes(), name.as_bytes())
}

fn splitlines_keepends(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                result.push(text[start..=i].to_string());
                i += 1;
                start = i;
            }
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    result.push(text[start..=i + 1].to_string());
                    i += 2;
                } else {
                    result.push(text[start..=i].to_string());
                    i += 1;
                }
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        result.push(text[start..].to_string());
    }
    result
}

fn reference_of(record: &Value) -> Value {
    json!({
        "repository": record["provenance"]["repository"],
        "id": record["id"],
        "revision_id": record["revision_id"],
        "source_revision": record["provenance"]["git_revision"],
        "status": "pending",
    })
}

struct RecordOpts {
    selector: String,
    scope: &'static str,
    activity: String,
    missing: Vec<String>,
}

impl RecordOpts {
    fn new() -> Self {
        Self {
            selector: "$".to_string(),
            scope: "unknown",
            activity: "unknown".to_string(),
            missing: Vec::new(),
        }
    }

    fn selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = selector.into();
        self
    }

    fn scope(mut self, scope: &'static str) -> Self {
        self.scope = scope;
        self
    }

    fn activity(mut self, activity: impl Into<String>) -> Self {
        self.activity = activity.into();
        self
    }

    fn missing(mut self, missing: &[&str]) -> Self {
        self.missing = missing.iter().map(|s| s.to_string()).collect();
        self
    }
}

#[allow(clippy::too_many_arguments)]
fn make_record(
    repository: &str,
    kind: &str,
    legacy_id: &str,
    payload: Value,
    provenance: Value,
    legacy: Value,
    activity: &str,
    scope: &str,
    limitations: Vec<String>,
    mut missingness: Vec<String>,
) -> Result<Value, ImportError> {
    let git_revision_present = provenance
        .get("git_revision")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    for (field, present) in [
        ("activity", activity != "unknown"),
        ("scope", scope != "unknown"),
        ("git-revision", git_revision_present),
    ] {
        if !present && !missingness.iter().any(|m| m == field) {
            missingness.push(field.to_string());
        }
    }
    let id = format!(
        "urn:research:{repository}:{kind}:{}",
        quote_safe_empty(legacy_id)
    );
    let mut record = json!({
        "schema_version": 1,
        "kind": kind,
        "id": id,
        "aliases": [legacy_id],
        "activity": activity,
        "scope": scope,
        "provenance": provenance,
        "limitations": limitations,
        "missingness": missingness,
        "legacy": legacy,
        "references": [],
        "presentation": {},
        "payload": payload,
    });
    let revision_id = orbit_research_contract::revision_digest(&record)?;
    record["revision_id"] = Value::String(revision_id);
    Ok(record)
}

struct Importer {
    root: PathBuf,
    adapter: String,
    repository: String,
    revision: Option<String>,
    inventory: Vec<Value>,
    records: Vec<Value>,
    files: Vec<Value>,
    before: BTreeMap<PathBuf, Option<String>>,
}

impl Importer {
    fn new(root: PathBuf, adapter: String, repository: String, revision: Option<String>) -> Self {
        Self {
            root,
            adapter,
            repository,
            revision,
            inventory: Vec::new(),
            records: Vec::new(),
            files: Vec::new(),
            before: BTreeMap::new(),
        }
    }

    fn track(&mut self, path: &Path) -> Result<Option<String>, ImportError> {
        if !contained(&self.root, path) {
            let shown = path.strip_prefix(&self.root).unwrap_or(path);
            return Err(value_error(format!(
                "source symlink escapes root: {}",
                shown.display()
            )));
        }
        if !self.before.contains_key(path) {
            let digest = if path.exists() {
                Some(file_digest(path)?)
            } else {
                None
            };
            self.before.insert(path.to_path_buf(), digest);
        }
        Ok(self.before.get(path).cloned().flatten())
    }

    fn provenance(&mut self, path: &Path, selector: &str) -> Result<Value, ImportError> {
        let digest = self.track(path)?;
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let blob = self.revision.as_ref().and_then(|rev| {
            git::run(&self.root, &["rev-parse", &format!("{rev}:{rel}")])
        });
        let actual = git::run(&self.root, &["hash-object", "--", &rel]);
        let working_tree = blob.is_none() || blob != actual;
        Ok(json!({
            "repository": self.repository,
            "git_revision": self.revision,
            "blob_oid": blob,
            "sha256": digest,
            "path": rel,
            "selector": selector,
            "historical": true,
            "working_tree": working_tree,
        }))
    }

    fn record(
        &mut self,
        path: &Path,
        kind: &str,
        ident: &str,
        payload: Value,
        raw: Value,
        opts: RecordOpts,
    ) -> Result<Value, ImportError> {
        let provenance = self.provenance(path, &opts.selector)?;
        make_record(
            &self.repository,
            kind,
            ident,
            payload,
            provenance,
            raw,
            &opts.activity,
            opts.scope,
            vec!["Historical candidate; owner reconciliation required.".to_string()],
            opts.missing,
        )
    }

    fn source_artifact(
        &mut self,
        path: &Path,
        raw: Value,
        selector: &str,
        role: &str,
    ) -> Result<Value, ImportError> {
        let digest = self
            .before
            .get(path)
            .cloned()
            .flatten()
            .expect("path tracked before source_artifact");
        let ident_full = digest_bytes(format!("{digest}{selector}").as_bytes());
        let ident = ident_full.strip_prefix("sha256:").unwrap_or(&ident_full);
        let media_type = if path.extension().and_then(|e| e.to_str()) == Some("json") {
            "application/json"
        } else {
            "text/plain"
        };
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let payload = json!({
            "role": role,
            "availability": "available",
            "snapshot_digest": digest,
            "locator": rel,
            "media_type": media_type,
        });
        let ident = ident.to_string();
        self.record(path, "artifact", &ident, payload, raw, RecordOpts::new().selector(selector))
    }

    fn item(
        &mut self,
        path: &Path,
        selector: &str,
        raw: Value,
        records: Vec<Value>,
        mut issues: Vec<(String, String)>,
    ) {
        let mut accepted = Vec::new();
        for record in records {
            let errors = orbit_research_contract::validate(&record, &[]);
            if !errors.is_empty() {
                issues.push(("invalid-candidate".to_string(), errors.join("; ")));
            } else {
                accepted.push(record);
            }
        }
        let rel = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let sha256 = self.before.get(path).cloned().flatten();
        let candidate_ids: Vec<Value> = accepted
            .iter()
            .filter_map(|r| r.get("id").cloned())
            .collect();
        let disposition = if issues.is_empty() { "mapped" } else { "exception" };
        let exceptions: Vec<Value> = issues
            .iter()
            .map(|(code, message)| json!({"code": code, "message": message}))
            .collect();
        self.inventory.push(json!({
            "key": format!("{rel}#{selector}"),
            "path": rel,
            "selector": selector,
            "sha256": sha256,
            "raw": raw,
            "disposition": disposition,
            "candidate_ids": candidate_ids,
            "exceptions": exceptions,
        }));
        self.records.extend(accepted);
    }

    fn json_file(&mut self, path: &Path, raw: Value) -> Result<(), ImportError> {
        let Some(object) = raw.as_object() else {
            self.item(
                path,
                "$",
                raw.clone(),
                vec![],
                vec![(
                    "ambiguous-shape".to_string(),
                    "Expected object; entire value retained.".to_string(),
                )],
            );
            let mut nested = Vec::new();
            nested_records(&raw, "$".to_string(), &mut nested);
            for (selector, child) in nested {
                self.item(
                    path,
                    &selector,
                    child.clone(),
                    vec![],
                    vec![(
                        "unmapped-member".to_string(),
                        "Member of unrecognized JSON container retained.".to_string(),
                    )],
                );
            }
            return Ok(());
        };
        let is_claims_json = self.adapter == "principia"
            && path.file_name().and_then(|n| n.to_str()) == Some("claims.json");
        let is_gate = self.adapter == "principia"
            && path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some("gates")
            && object.get("id").map(Value::is_string).unwrap_or(false);
        let is_sim = self.adapter == "orrery"
            && path.file_name().and_then(|n| n.to_str()) == Some("sim.json")
            && object.get("slug").map(Value::is_string).unwrap_or(false);
        let is_dataset =
            self.adapter == "astrolabe" && object.get("name").map(Value::is_string).unwrap_or(false);

        if is_claims_json {
            self.claims(path, raw.clone())?;
        } else if is_gate {
            let semantic = raw.clone();
            let semantic_digest = orbit_research_contract::protocol_digest(&semantic)?;
            let payload = json!({
                "semantic": semantic,
                "semantic_digest": semantic_digest,
                "freeze": "historical-unverified",
                "frozen_at": Value::Null,
                "freeze_evidence": Value::Null,
            });
            let ident = object.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            let rec = self.record(path, "protocol", &ident, payload, raw.clone(), RecordOpts::new())?;
            self.item(
                path,
                "$",
                raw.clone(),
                vec![rec],
                vec![(
                    "historical-protocol".to_string(),
                    "Gate text preserved; normative freeze and chronology require owner verification."
                        .to_string(),
                )],
            );
        } else if is_sim {
            let slug = object.get("slug").and_then(Value::as_str).unwrap_or_default().to_string();
            let title = object
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| slug.clone());
            let status = object.get("status").and_then(Value::as_str);
            let activity = status.filter(|s| is_activity(s)).unwrap_or("unknown");
            let payload = json!({"role": "program", "title": title});
            let rec = self.record(
                path,
                "program",
                &slug,
                payload,
                raw.clone(),
                RecordOpts::new().scope("simulation-under-assumptions").activity(activity),
            )?;
            self.item(
                path,
                "$",
                raw.clone(),
                vec![rec],
                vec![(
                    "catalog-not-run".to_string(),
                    "Simulation catalog is an activity; execution, protocol and evidence references require reconciliation."
                        .to_string(),
                )],
            );
        } else if is_dataset {
            self.dataset(path, raw.clone())?;
        } else {
            let role = if self.adapter == "orrery" { "result" } else { "source" };
            let rec = self.source_artifact(path, raw.clone(), "$", role)?;
            self.item(
                path,
                "$",
                raw.clone(),
                vec![rec],
                vec![(
                    "opaque-json".to_string(),
                    "Source artifact preserved; no generic result-to-assessment or execution inference."
                        .to_string(),
                )],
            );
        }

        let mut nested = Vec::new();
        nested_records(&raw, "$".to_string(), &mut nested);
        let claim_member_re = Regex::new(r"^\$/claims/\d+$").expect("valid regex");
        for (selector, child) in nested {
            if is_claims_json && claim_member_re.is_match(&selector) {
                continue;
            }
            self.item(
                path,
                &selector,
                child.clone(),
                vec![],
                vec![(
                    "retained-member".to_string(),
                    "Nested member retained verbatim in source; no independent scientific identity inferred."
                        .to_string(),
                )],
            );
        }
        Ok(())
    }

    fn claims(&mut self, path: &Path, raw: Value) -> Result<(), ImportError> {
        let mut records = Vec::new();
        let doc = raw.get("doc").and_then(Value::as_str).filter(|d| !d.is_empty());
        let title_is_str = raw.get("title").map(Value::is_string).unwrap_or(false);
        if let (Some(doc), true) = (doc, title_is_str) {
            let title = raw["title"].as_str().unwrap_or_default().to_string();
            let status = raw.get("status").and_then(Value::as_str);
            let activity = status.filter(|s| is_activity(s)).unwrap_or("unknown");
            let payload = json!({"role": "theory", "title": title});
            let rec = self.record(path, "program", doc, payload, raw.clone(), RecordOpts::new().activity(activity))?;
            records.push(rec);
        }
        self.item(
            path,
            "$",
            raw.clone(),
            records,
            vec![(
                "legacy-program-state".to_string(),
                "Theory status retained; exploratory/growing/refuted do not imply activity or scientific support."
                    .to_string(),
            )],
        );
        let Some(claims_list) = raw.get("claims").and_then(Value::as_array).cloned() else {
            self.item(
                path,
                "$/claims",
                raw.get("claims").cloned().unwrap_or(Value::Null),
                vec![],
                vec![(
                    "missing-claims".to_string(),
                    "claims must be an array; missingness retained.".to_string(),
                )],
            );
            return Ok(());
        };
        for (n, claim) in claims_list.iter().enumerate() {
            let loc = format!("$/claims/{n}");
            let id = claim.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
            let statement = claim.get("claim").and_then(Value::as_str).filter(|s| !s.is_empty());
            let (Some(id), Some(statement)) = (id, statement) else {
                self.item(
                    path,
                    &loc,
                    claim.clone(),
                    vec![],
                    vec![(
                        "malformed-claim".to_string(),
                        "Claim needs nonempty legacy id and exact statement.".to_string(),
                    )],
                );
                continue;
            };
            let id = id.to_string();
            let statement = statement.to_string();
            let kind = claim.get("kind").and_then(Value::as_str);
            let domain = match kind {
                Some("nature") => "nature",
                Some("derived") | Some("model-property") | Some("postulate") => "model",
                _ => "unknown",
            };
            let scope = if kind == Some("derived") { "derivation" } else { "unknown" };
            let role = if kind == Some("postulate") { "postulate" } else { "claim" };
            let payload = json!({"role": role, "statement": statement, "domain": domain});
            let missing_scope: &[&str] = if scope == "unknown" { &["scope"] } else { &[] };
            let rec = self.record(
                path,
                "claim",
                &id,
                payload,
                claim.clone(),
                RecordOpts::new().selector(loc.clone()).scope(scope).missing(missing_scope),
            )?;
            let status = claim.get("status").and_then(Value::as_str);
            let verdict = verdict_for(status);
            let evidence_rationale = claim
                .get("evidence")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "No source evidence rationale supplied.".to_string());
            let assessment_payload = json!({
                "claim": reference_of(&rec),
                "verdict": verdict,
                "inference": "historical",
                "controls": "unknown",
                "basis": "legacy-report",
                "rationale": evidence_rationale,
                "evidence": [],
                "legacy_verdict": status,
            });
            let assessment_ident = format!("{id}:legacy-verdict");
            let assessment = self.record(
                path,
                "assessment",
                &assessment_ident,
                assessment_payload,
                claim.clone(),
                RecordOpts::new().selector(loc.clone()).scope(scope).missing(&["verified-evidence", "controls"]),
            )?;
            self.item(
                path,
                &loc,
                claim.clone(),
                vec![rec, assessment],
                vec![(
                    "pending-evidence".to_string(),
                    "Exact claim/verdict retained; evidence, scope and controls are not independently verified."
                        .to_string(),
                )],
            );
        }
        Ok(())
    }

    fn dataset(&mut self, path: &Path, raw: Value) -> Result<(), ImportError> {
        let artifact_path = path.with_extension("parquet");
        let snapshot = self.track(&artifact_path)?;
        let kind = raw.get("kind").and_then(Value::as_str).filter(|k| !k.is_empty());
        let Some(kind) = kind else {
            self.item(
                path,
                "$",
                raw.clone(),
                vec![],
                vec![(
                    "ambiguous-dataset".to_string(),
                    "Dataset kind absent; path is not a substitute identity.".to_string(),
                )],
            );
            return Ok(());
        };
        let name = raw.get("name").and_then(Value::as_str).unwrap_or_default();
        let ident = format!("{kind}:{name}");
        let locator = artifact_path
            .strip_prefix(&self.root)
            .unwrap_or(&artifact_path)
            .to_string_lossy()
            .replace('\\', "/");
        let availability = if snapshot.is_some() { "available" } else { "missing" };
        let missing_snapshot: &[&str] = if snapshot.is_some() { &[] } else { &["dataset-snapshot"] };
        let payload = json!({
            "role": "dataset",
            "availability": availability,
            "snapshot_digest": snapshot,
            "locator": locator,
            "media_type": "application/vnd.apache.parquet",
        });
        let mut rec = self.record(path, "artifact", &ident, payload, raw.clone(), RecordOpts::new().missing(missing_snapshot))?;
        let mut issues = Vec::new();
        if snapshot.is_none() {
            issues.push((
                "missing-artifact".to_string(),
                "Sidecar survives but dataset bytes are unavailable.".to_string(),
            ));
        }
        if raw.get("lineage").map(truthy).unwrap_or(false) {
            issues.push((
                "pending-lineage".to_string(),
                "Legacy dataset names/timestamps are not exact revision pins; null parents remain missing."
                    .to_string(),
            ));
        }
        if raw.get("lineage").is_none() {
            rec["missingness"]
                .as_array_mut()
                .expect("missingness is an array")
                .push(Value::String("lineage-not-recorded".to_string()));
        }
        let new_digest = orbit_research_contract::revision_digest(&rec)?;
        rec["revision_id"] = Value::String(new_digest);
        self.item(path, "$", raw, vec![rec], issues);
        Ok(())
    }

    fn markdown(&mut self, path: &Path, content: String) -> Result<(), ImportError> {
        let source = self.source_artifact(path, Value::String(content.clone()), "$", "source")?;
        self.item(
            path,
            "$",
            Value::String(content.clone()),
            vec![source],
            vec![(
                "prose-preserved".to_string(),
                "Whole prose, frontmatter, ledgers and limitations retained; no inferred freeze or verdict."
                    .to_string(),
            )],
        );
        if self.adapter != "parallax" {
            return Ok(());
        }
        if self.parallax_document(path, &content)? {
            return Ok(());
        }
        let row_re = Regex::new(r"^\|\s*([RHE][0-9]+)\s*\|").expect("valid regex");
        let heading_re = Regex::new(r"^#{1,6}\s+[RHE][0-9]+\s*[:.]\s").expect("valid regex");
        let lines = splitlines_keepends(&content);
        for (idx, line) in lines.iter().enumerate() {
            let n = idx + 1;
            let Some(caps) = row_re.captures(line) else {
                continue;
            };
            let ident = caps.get(1).expect("group 1").as_str().to_string();
            let cells = split_unescaped_pipe(line);
            let loc = format!("line:{n}");
            if cells.len() < 4 || cells[2].trim().is_empty() {
                self.item(
                    path,
                    &loc,
                    Value::String(line.clone()),
                    vec![],
                    vec![(
                        "ambiguous-table".to_string(),
                        "Malformed R/H/E definition row retained.".to_string(),
                    )],
                );
            } else if ident.starts_with('H') {
                let statement = cells[2].trim().to_string();
                let payload = json!({"role": "hypothesis", "statement": statement, "domain": "empirical"});
                let rec = self.record(
                    path,
                    "claim",
                    &ident,
                    payload,
                    Value::String(line.clone()),
                    RecordOpts::new().selector(loc.clone()).scope("observation").missing(&["verdict", "activity", "protocol"]),
                )?;
                self.item(path, &loc, Value::String(line.clone()), vec![rec], vec![]);
            } else {
                self.item(
                    path,
                    &loc,
                    Value::String(line.clone()),
                    vec![],
                    vec![(
                        "unmapped-register-row".to_string(),
                        "R/E row retained; design prose does not establish a run or its outcome.".to_string(),
                    )],
                );
            }
        }
        for (idx, line) in lines.iter().enumerate() {
            let n = idx + 1;
            if heading_re.is_match(line) {
                self.item(
                    path,
                    &format!("heading:{n}"),
                    Value::String(line.clone()),
                    vec![],
                    vec![(
                        "unmapped-register-heading".to_string(),
                        "Definition/discussion heading retained with its full source document.".to_string(),
                    )],
                );
            }
        }
        Ok(())
    }

    fn parallax_document(&mut self, path: &Path, content: &str) -> Result<bool, ImportError> {
        let front_re = Regex::new(r"(?s)\A---\r?\n(.*?)\r?\n---\r?\n").expect("valid regex");
        let Some(front) = front_re.captures(content) else {
            return Ok(false);
        };
        let front_whole = front.get(0).expect("group 0").as_str().to_string();
        let front_body = front.get(1).expect("group 1").as_str();
        let field_re = Regex::new(r"^([a-z_]+):\s*(.*)$").expect("valid regex");
        let mut fields = Map::new();
        for line in front_body.lines() {
            let Some(caps) = field_re.captures(line) else {
                continue;
            };
            let key = caps.get(1).expect("group 1").as_str().to_string();
            let value_str = caps.get(2).expect("group 2").as_str();
            if fields.contains_key(&key) {
                self.item(
                    path,
                    "frontmatter",
                    Value::String(front_whole.clone()),
                    vec![],
                    vec![(
                        "ambiguous-frontmatter".to_string(),
                        "Duplicate key; no R/H/E identity inferred.".to_string(),
                    )],
                );
                return Ok(true);
            }
            let value = orbit_research_contract::parse_json(value_str.as_bytes())
                .unwrap_or_else(|_| Value::String(value_str.to_string()));
            fields.insert(key, value);
        }
        let rid_re = Regex::new(r"^R[0-9]+$").expect("valid regex");
        let rid = fields
            .get("research_id")
            .and_then(Value::as_str)
            .filter(|r| rid_re.is_match(r))
            .map(str::to_string);
        let Some(rid) = rid else {
            return Ok(false);
        };

        let heading_re = Regex::new(r"(?m)^## (.+)\r?$").expect("valid regex");
        let section_matches: Vec<(usize, usize, String)> = heading_re
            .captures_iter(content)
            .map(|cap| {
                let whole = cap.get(0).expect("group 0");
                let name = cap.get(1).expect("group 1").as_str().to_string();
                (whole.start(), whole.end(), name)
            })
            .collect();
        let mut sections = Map::new();
        for (idx, (_, end, name)) in section_matches.iter().enumerate() {
            let sec_end = if idx + 1 < section_matches.len() {
                section_matches[idx + 1].0
            } else {
                content.len()
            };
            if sections.contains_key(name) {
                self.item(
                    path,
                    "sections",
                    Value::String(content.to_string()),
                    vec![],
                    vec![(
                        "ambiguous-sections".to_string(),
                        "Repeated section heading; no scientific extraction.".to_string(),
                    )],
                );
                return Ok(true);
            }
            let body = content[*end..sec_end]
                .trim_matches(|c| c == '\r' || c == '\n')
                .to_string();
            sections.insert(name.clone(), Value::String(body));
        }
        let raw = json!({
            "frontmatter": Value::Object(fields.clone()),
            "sections": Value::Object(sections.clone()),
            "text": content,
        });
        let status = fields.get("status").and_then(Value::as_str);
        let activity = status.filter(|s| is_activity(s)).unwrap_or("unknown");
        let eid_re = Regex::new(r"^E[0-9]+$").expect("valid regex");
        let hid_re = Regex::new(r"^H[0-9]+$").expect("valid regex");
        let eid = fields
            .get("experiment_id")
            .and_then(Value::as_str)
            .filter(|e| eid_re.is_match(e))
            .map(str::to_string);
        let hid = fields
            .get("hypothesis_id")
            .and_then(Value::as_str)
            .filter(|h| hid_re.is_match(h))
            .map(str::to_string);
        let claim_section = sections.get("Claim").filter(|v| truthy(v)).is_some();

        let records: Vec<Value>;
        let mut issues = Vec::new();

        if let Some(eid) = eid {
            let ident = format!("{rid}/{eid}");
            let semantic = json!({
                "source_text": content,
                "frontmatter": Value::Object(fields.clone()),
                "sections": Value::Object(sections.clone()),
            });
            let semantic_digest = orbit_research_contract::protocol_digest(&semantic)?;
            let protocol_payload = json!({
                "semantic": semantic,
                "semantic_digest": semantic_digest,
                "freeze": "historical-unverified",
                "frozen_at": Value::Null,
                "freeze_evidence": Value::Null,
            });
            let protocol = self.record(
                path,
                "protocol",
                &format!("{ident}:protocol"),
                protocol_payload,
                raw.clone(),
                RecordOpts::new().selector("frontmatter").scope("observation").missing(&["independent-freeze-chronology"]),
            )?;
            let execution = status
                .filter(|s| matches!(*s, "completed" | "failed" | "cancelled" | "running" | "planned"))
                .unwrap_or("unknown");
            let run_payload = json!({
                "execution_status": execution,
                "controls": "unknown",
                "protocol": reference_of(&protocol),
                "result_artifacts": [],
            });
            let run = self.record(
                path,
                "experiment",
                &ident,
                run_payload,
                raw.clone(),
                RecordOpts::new().selector("frontmatter").scope("observation").missing(&["controls", "exact-result-artifacts", "assessment"]),
            )?;
            records = vec![protocol, run];
            issues.push((
                "historical-experiment".to_string(),
                "preregistered/status/outcome and full methodology preserved; revise/advance/reject are not scientific verdicts; no prospective chronology inferred."
                    .to_string(),
            ));
        } else if let (Some(hid), true) = (hid, claim_section) {
            let ident = format!("{rid}/{hid}");
            let statement = sections.get("Claim").and_then(Value::as_str).unwrap_or_default().to_string();
            let claim_payload = json!({"role": "hypothesis", "statement": statement, "domain": "empirical"});
            let claim = self.record(
                path,
                "claim",
                &ident,
                claim_payload,
                raw.clone(),
                RecordOpts::new().selector("section:Claim").scope("observation").activity(activity).missing(&["verified-evidence"]),
            )?;
            let outcome_value = fields.get("outcome").cloned().unwrap_or(Value::Null);
            let outcome_str = outcome_value.as_str();
            let verdict = verdict_for(outcome_str);
            let legacy_verdict = match &outcome_value {
                Value::String(s) => Value::String(s.clone()),
                _ => Value::Null,
            };
            let rationale = sections
                .get("Decision history")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "Exact source outcome retained; no independent assessment.".to_string());
            let assessment_payload = json!({
                "claim": reference_of(&claim),
                "verdict": verdict,
                "inference": "historical",
                "controls": "unknown",
                "basis": "legacy-report",
                "rationale": rationale,
                "evidence": [],
                "legacy_verdict": legacy_verdict,
            });
            let mut raw_status = raw.clone();
            raw_status["status"] = outcome_value;
            let assessment = self.record(
                path,
                "assessment",
                &format!("{ident}:legacy-verdict"),
                assessment_payload,
                raw_status,
                RecordOpts::new().selector("frontmatter:outcome").scope("observation").missing(&["verified-evidence", "controls"]),
            )?;
            records = vec![claim, assessment];
            issues.push((
                "historical-hypothesis".to_string(),
                "Exact Claim and outcome retained; revised is unknown, archived is not silently translated into a scientific verdict."
                    .to_string(),
            ));
        } else if path.file_name().and_then(|n| n.to_str()) == Some("README.md")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{rid}-")))
            && fields
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.trim().is_empty())
        {
            let title = fields.get("title").and_then(Value::as_str).unwrap_or_default().to_string();
            let payload = json!({"role": "program", "title": title});
            let rec = self.record(
                path,
                "program",
                &rid,
                payload,
                raw.clone(),
                RecordOpts::new().selector("frontmatter").activity(activity).scope("observation"),
            )?;
            records = vec![rec];
        } else {
            return Ok(false);
        }
        self.item(path, "research-definition", raw, records, issues);
        Ok(true)
    }

    fn database(&mut self, path: &Path) -> Result<(), ImportError> {
        let suffixes = ["-wal", "-shm", "-journal"];
        let sidecars: Vec<PathBuf> = suffixes
            .iter()
            .map(|suffix| {
                let mut s = path.as_os_str().to_os_string();
                s.push(suffix);
                PathBuf::from(s)
            })
            .collect();
        for p in &sidecars {
            self.track(p)?;
        }
        if sidecars[2].exists() {
            self.item(
                path,
                "$",
                Value::Null,
                vec![],
                vec![(
                    "sqlite-journal".to_string(),
                    "Rollback journal present; request a quiescent owner snapshot.".to_string(),
                )],
            );
            return Ok(());
        }
        let tmp = tempfile::Builder::new()
            .prefix("orbit-research-sqlite-")
            .tempdir()?;
        let dest = tmp.path().join("snapshot.db");
        let dest_wal = {
            let mut s = dest.as_os_str().to_os_string();
            s.push("-wal");
            PathBuf::from(s)
        };
        for (source, target) in [(path.to_path_buf(), dest.clone()), (sidecars[0].clone(), dest_wal.clone())] {
            if source.exists() {
                fs::copy(&source, &target)?;
                let digest = file_digest(&target)?;
                let expected = self.before.get(&source).cloned().flatten();
                if Some(digest) != expected {
                    return Err(value_error("source changed during SQLite snapshot"));
                }
            }
        }
        let mut all_paths = vec![path.to_path_buf()];
        all_paths.extend(sidecars.iter().cloned());
        for p in &all_paths {
            let digest = if p.exists() { Some(file_digest(p)?) } else { None };
            let expected = self.before.get(p).cloned().flatten();
            if digest != expected {
                return Err(value_error("source changed during SQLite snapshot"));
            }
        }
        let conn = rusqlite::Connection::open_with_flags(
            format!("file:{}?mode=ro", dest.display()),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.execute("PRAGMA query_only=ON", [])?;
        let mut tables: Vec<(String, Option<String>)> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            for r in rows {
                tables.push(r?);
            }
        }
        let tables_value: Vec<Value> = tables
            .iter()
            .map(|(name, sql)| json!({"name": name, "sql": sql}))
            .collect();
        self.item(
            path,
            "$",
            Value::Array(tables_value),
            vec![],
            vec![(
                "sqlite-schema".to_string(),
                "All user table definitions retained; every row separately inventoried.".to_string(),
            )],
        );
        for (name, _sql) in &tables {
            let quoted = format!("\"{}\"", name.replace('"', "\"\""));
            let mut stmt = conn.prepare(&format!("SELECT * FROM {quoted}"))?;
            let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let mut rows = stmt.query([])?;
            let mut n = 0usize;
            while let Some(row) = rows.next()? {
                let mut raw = Map::new();
                for (i, col) in column_names.iter().enumerate() {
                    let value_ref = row.get_ref(i)?;
                    raw.insert(col.clone(), sqlite_cell_to_value(value_ref));
                }
                let has_nonfinite = column_names.iter().enumerate().any(|(i, _)| {
                    matches!(row.get_ref(i), Ok(ValueRef::Real(f)) if !f.is_finite())
                });
                let loc = format!("table:{name}/row:{n}");
                n += 1;
                if has_nonfinite {
                    for (i, col) in column_names.iter().enumerate() {
                        if let Ok(ValueRef::Real(f)) = row.get_ref(i)
                            && !f.is_finite()
                        {
                            let repr = if f.is_nan() {
                                "nan"
                            } else if f > 0.0 {
                                "inf"
                            } else {
                                "-inf"
                            };
                            raw[col] = json!({"sqlite_nonfinite": repr});
                        }
                    }
                    self.item(
                        path,
                        &loc,
                        Value::Object(raw),
                        vec![],
                        vec![(
                            "nonfinite-sqlite-row".to_string(),
                            "Nonfinite SQL value retained with explicit encoding; no candidate emitted.".to_string(),
                        )],
                    );
                    continue;
                }
                let raw_value = Value::Object(raw.clone());
                if (name == "trade_intents" || name == "research_intents")
                    && raw.get("id").and_then(Value::as_str).is_some()
                    && raw.get("hypothesis").and_then(Value::as_str).is_some_and(|s| !s.is_empty())
                {
                    let id = raw.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let hypothesis = raw.get("hypothesis").and_then(Value::as_str).unwrap_or_default().to_string();
                    let prefix = if name == "research_intents" { "research-journal:" } else { "journal:" };
                    let ident = format!("{prefix}{id}");
                    let payload = json!({"role": "hypothesis", "statement": hypothesis, "domain": "empirical"});
                    let rec = self.record(
                        path,
                        "claim",
                        &ident,
                        payload,
                        raw_value.clone(),
                        RecordOpts::new().selector(loc.clone()).scope("observation").missing(&["verified-freeze", "verdict"]),
                    )?;
                    let mut records = vec![rec];
                    if name == "research_intents" {
                        let semantic = raw_value.clone();
                        let semantic_digest = orbit_research_contract::protocol_digest(&semantic)?;
                        let protocol_payload = json!({
                            "semantic": semantic,
                            "semantic_digest": semantic_digest,
                            "freeze": "historical-unverified",
                            "frozen_at": Value::Null,
                            "freeze_evidence": Value::Null,
                        });
                        let protocol = self.record(
                            path,
                            "protocol",
                            &ident,
                            protocol_payload,
                            raw_value.clone(),
                            RecordOpts::new().selector(loc.clone()).scope("observation").missing(&["independent-freeze-chronology"]),
                        )?;
                        records.push(protocol);
                    }
                    self.item(
                        path,
                        &loc,
                        raw_value,
                        records,
                        vec![(
                            "historical-intent".to_string(),
                            "Intent timestamp/rules retained; no prospective preregistration fabricated.".to_string(),
                        )],
                    );
                } else if name == "trade_outcomes" || name == "research_outcomes" {
                    let key = if name == "research_outcomes" { "experiment_id" } else { "trade_id" };
                    if let Some(idv) = raw.get(key).and_then(Value::as_str) {
                        let prefix = if name == "research_outcomes" { "research-journal:" } else { "journal:" };
                        let ident = format!("{prefix}{idv}");
                        let payload = json!({
                            "execution_status": "completed",
                            "controls": "unknown",
                            "protocol": Value::Null,
                            "result_artifacts": [],
                        });
                        let rec = self.record(
                            path,
                            "experiment",
                            &ident,
                            payload,
                            raw_value.clone(),
                            RecordOpts::new().selector(loc.clone()).scope("observation").missing(&["protocol", "controls", "assessment"]),
                        )?;
                        self.item(
                            path,
                            &loc,
                            raw_value,
                            vec![rec],
                            vec![(
                                "outcome-not-support".to_string(),
                                "Recorded outcome is execution history; free-text result is not a scientific verdict."
                                    .to_string(),
                            )],
                        );
                    } else {
                        self.item(
                            path,
                            &loc,
                            raw_value,
                            vec![],
                            vec![(
                                "unknown-journal-row".to_string(),
                                "Unknown table/row shape retained without invented mapping.".to_string(),
                            )],
                        );
                    }
                } else {
                    self.item(
                        path,
                        &loc,
                        raw_value,
                        vec![],
                        vec![(
                            "unknown-journal-row".to_string(),
                            "Unknown table/row shape retained without invented mapping.".to_string(),
                        )],
                    );
                }
            }
        }
        Ok(())
    }

    fn run(&mut self, paths: Vec<PathBuf>, patterns: Vec<String>) -> Result<Value, ImportError> {
        let had_paths = !paths.is_empty();
        let db_suffixes = ["db", "sqlite", "sqlite3"];
        let any_sqlite = paths.iter().any(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| db_suffixes.contains(&e))
        });
        for path in &paths {
            if !contained(&self.root, path) {
                self.item(
                    path,
                    "$",
                    Value::Null,
                    vec![],
                    vec![(
                        "outside-root".to_string(),
                        "Symlink target outside source root was not read.".to_string(),
                    )],
                );
                continue;
            }
            let prov = self.provenance(path, "$")?;
            self.files.push(json!({
                "path": prov["path"],
                "sha256": prov["sha256"],
                "blob_oid": prov["blob_oid"],
                "working_tree": prov["working_tree"],
            }));
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
            let result: Result<(), ImportError> = (|| {
                if db_suffixes.contains(&ext.as_str()) {
                    self.database(path)
                } else if fs::metadata(path)?.len() > MAX_METADATA_BYTES {
                    self.item(
                        path,
                        "$",
                        Value::Null,
                        vec![],
                        vec![(
                            "large-metadata".to_string(),
                            "Metadata exceeds 8 MiB; immutable digest retained; owner must select a smaller export."
                                .to_string(),
                        )],
                    );
                    Ok(())
                } else if ext == "json" {
                    let bytes = fs::read(path)?;
                    match orbit_research_contract::parse_json(&bytes) {
                        Ok(value) => self.json_file(path, value),
                        Err(e) => {
                            self.item(
                                path,
                                "read-error",
                                Value::Null,
                                vec![],
                                vec![("read-error".to_string(), e.to_string())],
                            );
                            Ok(())
                        }
                    }
                } else if ext == "md" {
                    let bytes = fs::read(path)?;
                    match String::from_utf8(bytes) {
                        Ok(text) => self.markdown(path, text),
                        Err(e) => {
                            self.item(
                                path,
                                "read-error",
                                Value::Null,
                                vec![],
                                vec![("read-error".to_string(), e.to_string())],
                            );
                            Ok(())
                        }
                    }
                } else {
                    self.item(
                        path,
                        "$",
                        Value::Null,
                        vec![],
                        vec![(
                            "unsupported-format".to_string(),
                            "Selected file digest preserved; unsupported format.".to_string(),
                        )],
                    );
                    Ok(())
                }
            })();
            if let Err(e) = result {
                self.item(path, "read-error", Value::Null, vec![], vec![("read-error".to_string(), e.to_string())]);
            }
        }
        if !had_paths {
            let discovery_path = self.root.join("(discovery)");
            self.item(
                &discovery_path,
                "$",
                Value::Null,
                vec![],
                vec![(
                    "no-inputs".to_string(),
                    "No files matched the documented discovery patterns.".to_string(),
                )],
            );
        }
        if self.adapter == "parallax" && !any_sqlite {
            let journal_path = self.root.join("(journal)");
            self.item(
                &journal_path,
                "$",
                Value::Null,
                vec![],
                vec![(
                    "journal-unavailable".to_string(),
                    "No SQLite journal selected/discovered; no intent/outcome history invented.".to_string(),
                )],
            );
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for r in &self.records {
            if let Some(id) = r.get("id").and_then(Value::as_str) {
                *counts.entry(id.to_string()).or_insert(0) += 1;
            }
        }
        let collisions: HashSet<String> = counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, _)| id)
            .collect();
        if !collisions.is_empty() {
            self.records
                .retain(|r| !r.get("id").and_then(Value::as_str).is_some_and(|id| collisions.contains(id)));
            for item in self.inventory.iter_mut() {
                let candidate_ids = item["candidate_ids"].as_array().cloned().unwrap_or_default();
                let has_collision = candidate_ids
                    .iter()
                    .any(|c| c.as_str().is_some_and(|id| collisions.contains(id)));
                if has_collision {
                    let filtered: Vec<Value> = candidate_ids
                        .into_iter()
                        .filter(|c| !c.as_str().is_some_and(|id| collisions.contains(id)))
                        .collect();
                    item["candidate_ids"] = Value::Array(filtered);
                    item["disposition"] = Value::String("exception".to_string());
                    item["exceptions"]
                        .as_array_mut()
                        .expect("exceptions is an array")
                        .push(json!({
                            "code": "identity-collision",
                            "message": "Repeated namespaced identity requires owner disambiguation; raw records retained.",
                        }));
                }
            }
        }

        for (path, before) in self.before.iter() {
            let current = if path.exists() { Some(file_digest(path)?) } else { None };
            if &current != before {
                let shown = path.strip_prefix(&self.root).unwrap_or(path);
                return Err(value_error(format!("source changed during import: {}", shown.display())));
            }
        }
        let selected_files: HashSet<String> = self
            .files
            .iter()
            .filter_map(|f| f.get("path").and_then(Value::as_str).map(str::to_string))
            .collect();
        let before_snapshot: Vec<(PathBuf, Option<String>)> =
            self.before.iter().map(|(p, d)| (p.clone(), d.clone())).collect();
        for (path, before) in before_snapshot {
            if before.is_some() {
                let rel = path.strip_prefix(&self.root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if !selected_files.contains(&rel) {
                    let prov = self.provenance(&path, "$")?;
                    self.files.push(json!({
                        "path": prov["path"],
                        "sha256": prov["sha256"],
                        "blob_oid": prov["blob_oid"],
                        "working_tree": prov["working_tree"],
                    }));
                }
            }
        }
        let head = git::run(&self.root, &["rev-parse", "HEAD"]);
        if head != self.revision {
            return Err(value_error("source Git revision changed during import"));
        }

        let discovered = self.inventory.len();
        let mapped = self
            .inventory
            .iter()
            .filter(|i| i["disposition"].as_str() == Some("mapped"))
            .count();
        let exceptions = self
            .inventory
            .iter()
            .filter(|i| i["disposition"].as_str() == Some("exception"))
            .count();
        let aliases: Vec<Value> = self
            .records
            .iter()
            .flat_map(|r| {
                r["aliases"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| json!({"alias": a, "id": r["id"]}))
            })
            .collect();
        let manifest = json!({
            "schema_version": 1,
            "kind": "manifest",
            "repositories": [{"id": self.repository, "git_revision": self.revision}],
            "references": self.records.iter().map(reference_of).collect::<Vec<_>>(),
        });
        Ok(json!({
            "schema_version": 1,
            "kind": "import-report",
            "adapter": self.adapter,
            "repository": self.repository,
            "source_revision": self.revision,
            "dry_run": true,
            "discovery": patterns,
            "files": self.files,
            "inventory": self.inventory,
            "candidates": self.records,
            "aliases": aliases,
            "manifest": manifest,
            "counts": {
                "discovered": discovered,
                "mapped": mapped,
                "exceptions": exceptions,
            },
            "source_unchanged": true,
        }))
    }
}

fn nested_records<'a>(value: &'a Value, selector: String, out: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter() {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                nested_records(child, format!("{selector}/{escaped}"), out);
            }
        }
        Value::Array(items) => {
            for (n, child) in items.iter().enumerate() {
                let loc = format!("{selector}/{n}");
                if child.is_object() {
                    out.push((loc.clone(), child));
                }
                nested_records(child, loc, out);
            }
        }
        _ => {}
    }
}

fn split_unescaped_pipe(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut prev_backslash = false;
    for c in line.chars() {
        if c == '|' && !prev_backslash {
            result.push(current.clone());
            current.clear();
        } else {
            current.push(c);
        }
        prev_backslash = c == '\\';
    }
    result.push(current);
    result
}

fn sqlite_cell_to_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => {
            if f.is_finite() {
                serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
            } else {
                // Callers re-check finiteness and overwrite non-finite cells
                // before the value is ever serialized.
                Value::Null
            }
        }
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => json!({"sqlite_blob_hex": hex_encode(b)}),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Read-only dry-run import. Never writes a scientific record; `--select`
/// (via `selected`) replaces the default discovery boundary.
pub fn import_source(
    root: &Path,
    adapter: &str,
    repository: &str,
    selected: Option<&[String]>,
    expected_revision: Option<&str>,
) -> Result<Value, ImportError> {
    let root = root
        .canonicalize()
        .map_err(|_| value_error("source must be a directory and adapter must be supported"))?;
    if !root.is_dir() || !ADAPTERS.contains(&adapter) {
        return Err(value_error("source must be a directory and adapter must be supported"));
    }
    repository
        .parse::<RepositoryId>()
        .map_err(|_| value_error("repository identity must be an explicit stable namespace, not a path"))?;
    let revision = git::run(&root, &["rev-parse", "HEAD"]);
    let checkout = git::run(&root, &["rev-parse", "--show-toplevel"]);
    if let Some(checkout) = checkout {
        let checkout_resolved = resolve_best_effort(Path::new(&checkout));
        if checkout_resolved != root {
            return Err(value_error(
                "Git source root must be the checkout root; use --select for nested inputs",
            ));
        }
    }
    if let Some(expected) = expected_revision
        && Some(expected) != revision.as_deref()
    {
        return Err(value_error(format!(
            "source revision mismatch: expected {expected}, inspected {}",
            revision.as_deref().unwrap_or("None")
        )));
    }
    let (paths, patterns) = discover(&root, adapter, selected)?;
    let mut importer = Importer::new(root, adapter.to_string(), repository.to_string(), revision);
    importer.run(paths, patterns)
}

/// Create a new report file only; refuse source-root, existing-file,
/// hardlink and source-escaping-symlink destinations.
pub fn write_report(report: &Value, output: &Path, source_roots: &[PathBuf]) -> Result<(), ImportError> {
    let resolved = resolve_best_effort(output);
    for root in source_roots {
        let root_resolved = resolve_best_effort(root);
        if resolved.starts_with(&root_resolved) {
            return Err(value_error("report output must be outside every source root"));
        }
    }
    let pretty = serde_json::to_string_pretty(report)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(output)?;
    file.write_all(pretty.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}
