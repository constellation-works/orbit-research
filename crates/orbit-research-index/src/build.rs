//! The disposable projection itself. Ports `index.py::_documents` and `index.py::project`.
//!
//! Reconciliation is computed on private copies of the owner records: canonical
//! `reference.status` values on disk are never edited, and every reconciliation decision
//! (pending vs. resolved, current vs. superseded, eligible vs. conflicting) is derived fresh
//! from the currently supplied documents rather than trusted from an authored status field.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use orbit_research_contract::record_references;
use orbit_research_owner::Checkout;
use serde_json::{Value, json};

use crate::config::{load_config, valid_reference_shape};
use crate::{IndexConfig, IndexError, Problem, Result, require, text};

/// `(repository, id, revision_id, source_revision)` — the exact snapshot key everything in
/// this crate keys on.
pub type Pin4 = (String, String, String, Option<String>);

pub struct Projection {
    pub value: Value,
}

pub fn build_projection(config_path: &Path) -> Result<(Projection, IndexConfig)> {
    let config = load_config(config_path)?;
    let (pairs, manifests, inventories, mut observations) = load_documents(&config.paths)?;
    require(
        !manifests.is_empty(),
        "at least one explicit owner manifest is required",
    )?;
    let value = build_nodes(
        &pairs,
        &manifests,
        inventories,
        &config.roots,
        &mut observations,
    )?;
    Ok((Projection { value }, config))
}

pub(crate) fn pin(reference: &Value) -> Pin4 {
    (
        text(reference, "repository").unwrap_or_default().to_owned(),
        text(reference, "id").unwrap_or_default().to_owned(),
        text(reference, "revision_id").unwrap_or_default().to_owned(),
        reference
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    )
}

fn record_pin(record: &Value) -> Pin4 {
    let provenance = record.get("provenance");
    (
        provenance
            .and_then(|p| text(p, "repository"))
            .unwrap_or_default()
            .to_owned(),
        text(record, "id").unwrap_or_default().to_owned(),
        text(record, "revision_id").unwrap_or_default().to_owned(),
        provenance
            .and_then(|p| p.get("git_revision"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    )
}

pub(crate) fn key_of(pin: &Pin4) -> Result<String> {
    let bytes = orbit_research_contract::canonical_json(&json!([pin.0, pin.1, pin.2, pin.3]))?;
    Ok(crate::digest_bytes(&bytes)[7..].to_owned())
}

// ---------------------------------------------------------------------------------------
// Document loading: schema/structural validation per document, isolated from every other
// document's context. Reference resolution is deferred entirely to `build_nodes`, which
// never trusts a reference's own `status` field.
// ---------------------------------------------------------------------------------------

type Documents = (Vec<(Value, String)>, Vec<Value>, Vec<Value>, HashMap<PathBuf, String>);

fn load_documents(paths: &[PathBuf]) -> Result<Documents> {
    let mut records = Vec::new();
    let mut manifests = Vec::new();
    let mut inventories = Vec::new();
    let mut observations = HashMap::new();
    let mut problems = Vec::new();
    for path in paths {
        match load_one_document(path, &mut observations) {
            Ok((batch, ms, inventory)) => {
                let source = path.to_string_lossy().into_owned();
                records.extend(batch.into_iter().map(|record| (record, source.clone())));
                manifests.extend(ms);
                if let Some(inventory) = inventory {
                    inventories.push(inventory);
                }
            }
            Err(error) => problems.push(Problem {
                source: path.to_string_lossy().into_owned(),
                reason: error.to_string(),
            }),
        }
    }
    if !problems.is_empty() {
        return Err(IndexError::Build(problems));
    }
    Ok((records, manifests, inventories, observations))
}

/// One document's batch of records, the manifests it carries, and an import-report
/// inventory entry, or an error describing exactly why the document was refused.
fn load_one_document(
    path: &Path,
    observations: &mut HashMap<PathBuf, String>,
) -> Result<(Vec<Value>, Vec<Value>, Option<Value>)> {
    require(
        !path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink()),
        "document symlink is forbidden",
    )?;
    let data = fs::read(path)?;
    observations.insert(path.to_path_buf(), crate::digest_bytes(&data));
    let doc = orbit_research_contract::parse_json(&data)?;
    let kind = text(&doc, "kind").unwrap_or_default().to_owned();
    let mut errors = Vec::new();
    let (batch, manifests, inventory): (Vec<Value>, Vec<Value>, Option<Value>) = match kind
        .as_str()
    {
        "manifest" => (vec![], vec![doc.clone()], None),
        "export" => {
            let records_ok = doc.get("records").is_some_and(Value::is_array);
            let manifests_ok = doc.get("manifests").is_some_and(Value::is_array);
            if !records_ok || !manifests_ok {
                errors.push("export requires records and manifests arrays".to_owned());
                (vec![], vec![], None)
            } else {
                let batch = doc["records"].as_array().expect("checked above").clone();
                let ms = doc["manifests"].as_array().expect("checked above").clone();
                let pinned: HashSet<Pin4> = ms
                    .iter()
                    .flat_map(|m| {
                        m.get("references")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                    })
                    .map(pin)
                    .collect();
                let expected: HashSet<Pin4> = batch.iter().map(record_pin).collect();
                if pinned != expected {
                    errors
                        .push("export manifests do not account for exact record snapshots".to_owned());
                }
                (batch, ms, None)
            }
        }
        "import-report" => {
            let batch = doc
                .get("candidates")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !doc.get("candidates").is_some_and(Value::is_array) {
                errors.push("import-report requires a candidates array".to_owned());
            }
            let manifest = doc.get("manifest").cloned().unwrap_or(Value::Null);
            let inventory = Some(json!({
                "source": path.to_string_lossy(),
                "counts": doc.get("counts"),
                "inventory": doc.get("inventory"),
            }));
            (batch, vec![manifest], inventory)
        }
        _ => (vec![doc.clone()], vec![], None),
    };
    for manifest in &manifests {
        errors.extend(manifest_shape_errors(manifest));
    }
    for record in &batch {
        errors.extend(orbit_research_contract::validate_structural(record));
    }
    require(errors.is_empty(), errors.join("; "))?;
    Ok((batch, manifests, inventory))
}

/// Best-effort structural check for a standalone manifest document: required fields present
/// with plausible types. Repository-pin consistency for a "resolved" reference is derived
/// independently by `build_nodes`'s `declared` computation, not trusted here.
fn manifest_shape_errors(manifest: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if manifest.get("schema_version").and_then(Value::as_u64) != Some(1) {
        errors.push("manifest requires schema_version 1".to_owned());
    }
    if text(manifest, "kind") != Some("manifest") {
        errors.push("manifest requires kind \"manifest\"".to_owned());
    }
    let repositories_ok = manifest
        .get("repositories")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().all(|repo| text(repo, "id").is_some()));
    if !repositories_ok {
        errors.push("manifest repositories require a string id".to_owned());
    }
    let references_ok = manifest
        .get("references")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().all(valid_reference_shape));
    if !references_ok {
        errors.push("manifest references require repository/id/revision_id/status".to_owned());
    }
    errors
}

// ---------------------------------------------------------------------------------------
// Projection: group records by exact snapshot, resolve heads/edges, propagate pending
// dependencies and conflicting assessments to a fixed point.
// ---------------------------------------------------------------------------------------

struct Edge {
    reference: Value,
    target: Option<String>,
    status: &'static str,
    reason: String,
}

impl Edge {
    fn to_value(&self) -> Value {
        json!({
            "reference": self.reference,
            "target": self.target,
            "status": self.status,
            "reason": self.reason,
        })
    }
}

struct Node {
    key: String,
    pin: Pin4,
    record: Value,
    variants: Vec<Value>,
    documents: Vec<String>,
    reasons: BTreeSet<String>,
    edges: Vec<Edge>,
    assessments: Vec<String>,
    heads: Vec<String>,
    history: &'static str,
    reconciliation: &'static str,
    confirmation: String,
}

impl Node {
    fn axes(&self) -> Value {
        let payload = self.record.get("payload").cloned().unwrap_or(Value::Null);
        json!({
            "activity": self.record.get("activity").cloned().unwrap_or(Value::Null),
            "execution": payload.get("execution_status").cloned().unwrap_or_else(|| json!("not-applicable")),
            "verdict": payload.get("verdict").cloned().unwrap_or_else(|| json!("not-assessed")),
            "controls": payload.get("controls").cloned().unwrap_or_else(|| json!("not-applicable")),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "key": self.key,
            "pin": [self.pin.0, self.pin.1, self.pin.2, self.pin.3],
            "record": self.record,
            "variants": self.variants,
            "documents": self.documents,
            "reasons": self.reasons.iter().cloned().collect::<Vec<_>>(),
            "edges": self.edges.iter().map(Edge::to_value).collect::<Vec<_>>(),
            "assessments": self.assessments,
            "heads": self.heads,
            "history": self.history,
            "reconciliation": self.reconciliation,
            "axes": self.axes(),
            "confirmation": self.confirmation,
        })
    }
}

fn declared_pins(manifests: &[Value]) -> HashSet<Pin4> {
    let mut declared = HashSet::new();
    for manifest in manifests {
        let valid: HashSet<(Option<&str>, Option<&str>)> = manifest
            .get("repositories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|repo| (text(repo, "id"), repo.get("git_revision").and_then(Value::as_str)))
            .collect();
        for reference in manifest
            .get("references")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = (
                text(reference, "repository"),
                reference.get("source_revision").and_then(Value::as_str),
            );
            if valid.contains(&key) {
                declared.insert(pin(reference));
            }
        }
    }
    declared
}

fn build_nodes(
    pairs: &[(Value, String)],
    manifests: &[Value],
    inventories: Vec<Value>,
    roots: &BTreeMap<String, Checkout>,
    observations: &mut HashMap<PathBuf, String>,
) -> Result<Value> {
    let declared = declared_pins(manifests);

    let mut grouped: BTreeMap<Pin4, BTreeMap<String, Value>> = BTreeMap::new();
    let mut locations: BTreeMap<Pin4, BTreeSet<String>> = BTreeMap::new();
    for (record, source) in pairs {
        let k = record_pin(record);
        let canonical = String::from_utf8_lossy(&orbit_research_contract::canonical_json(record)?)
            .into_owned();
        grouped.entry(k.clone()).or_default().insert(canonical, record.clone());
        locations.entry(k).or_default().insert(source.clone());
    }

    let mut cache: HashMap<(String, String, String, String), Vec<String>> = HashMap::new();
    let mut nodes: BTreeMap<Pin4, Node> = BTreeMap::new();
    for (k, variants_map) in &grouped {
        let variants: Vec<Value> = variants_map.values().cloned().collect();
        let r = variants[0].clone();
        let mut reasons = source_state(&r, roots, &mut cache, observations);
        if variants.len() > 1 {
            reasons.push("conflicting records share this exact identity/revision/source pin".to_owned());
        }
        if !declared.contains(k) {
            reasons.push("exact record/source pin is absent from supplied manifests".to_owned());
        }
        if text(&r, "kind") == Some("artifact") {
            reasons.extend(artifact_state(&r, roots, observations));
        }
        if let Some(code) = record_code(&r) {
            let repository = text(code, "repository").unwrap_or_default();
            let revision = text(code, "git_revision").unwrap_or_default();
            let ok = roots.get(repository).is_some_and(|root| {
                orbit_research_owner::git::full_revision(revision)
                    && root.text(&["cat-file", "-t", revision]).as_deref() == Some("commit")
            });
            if !ok {
                reasons.push("exact code commit is not available in mapped repository".to_owned());
            }
        }
        for missing in r.get("missingness").and_then(Value::as_array).into_iter().flatten() {
            if let Some(missing) = missing.as_str() {
                reasons.push(format!("owner missingness: {missing}"));
            }
        }
        nodes.insert(
            k.clone(),
            Node {
                key: key_of(k)?,
                pin: k.clone(),
                record: r,
                variants: if variants.len() > 1 { variants } else { vec![] },
                documents: locations.get(k).cloned().unwrap_or_default().into_iter().collect(),
                reasons: reasons.into_iter().collect(),
                edges: vec![],
                assessments: vec![],
                heads: vec![],
                history: "current",
                reconciliation: "resolved",
                confirmation: "not-current".to_owned(),
            },
        );
    }

    // Only explicit supersession can select a head; file order and timestamps cannot.
    let mut by_id: HashMap<(String, String), BTreeSet<String>> = HashMap::new();
    let mut superseded: HashMap<(String, String), BTreeSet<String>> = HashMap::new();
    for (k, n) in &nodes {
        by_id.entry((k.0.clone(), k.1.clone())).or_default().insert(k.2.clone());
        if n.reasons.is_empty() {
            for reference in n
                .record
                .get("authorship")
                .and_then(|a| a.get("supersedes"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if nodes.contains_key(&pin(reference)) {
                    superseded
                        .entry((
                            text(reference, "repository").unwrap_or_default().to_owned(),
                            text(reference, "id").unwrap_or_default().to_owned(),
                        ))
                        .or_default()
                        .insert(text(reference, "revision_id").unwrap_or_default().to_owned());
                }
            }
        }
    }
    let keys: Vec<Pin4> = nodes.keys().cloned().collect();
    for k in &keys {
        let id_key = (k.0.clone(), k.1.clone());
        let all: BTreeSet<String> = by_id.get(&id_key).cloned().unwrap_or_default();
        let gone: BTreeSet<String> = superseded.get(&id_key).cloned().unwrap_or_default();
        let heads_set: BTreeSet<String> = all.difference(&gone).cloned().collect();
        let node = nodes.get_mut(k).expect("key from nodes.keys()");
        node.heads = heads_set.iter().cloned().collect();
        let historical = node
            .record
            .get("provenance")
            .and_then(|p| p.get("historical"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        node.history = if historical {
            "historical"
        } else if !heads_set.contains(&k.2) {
            "superseded"
        } else {
            "current"
        };
        if !historical && heads_set.len() > 1 {
            node.reasons.insert("conflicting revision heads require an owner decision".to_owned());
        }
    }

    // Edges and assessment links: read every node's dependencies before mutating any node.
    let mut computed_edges: HashMap<Pin4, Vec<Edge>> = HashMap::new();
    let mut assessment_links: Vec<(String, Pin4)> = Vec::new();
    for (k, n) in &nodes {
        let mut edges = Vec::new();
        for reference in record_references(&n.record) {
            let target_pin = pin(reference);
            let target = nodes.get(&target_pin);
            let mut reason = match target {
                None => "exact target is absent".to_owned(),
                Some(target) => target.reasons.iter().cloned().collect::<Vec<_>>().join("; "),
            };
            if reason.is_empty()
                && reference
                    .get("source_revision")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                reason = "target source revision is unpinned".to_owned();
            }
            edges.push(Edge {
                reference: reference.clone(),
                target: target.map(|target| target.key.clone()),
                status: if reason.is_empty() { "resolved" } else { "pending" },
                reason,
            });
        }
        computed_edges.insert(k.clone(), edges);
        if text(&n.record, "kind") == Some("assessment")
            && let Some(claim) = n.record.get("payload").and_then(|payload| payload.get("claim"))
        {
            assessment_links.push((n.key.clone(), pin(claim)));
        }
    }
    for (k, edges) in computed_edges {
        nodes.get_mut(&k).expect("computed for every node").edges = edges;
    }
    for (assessment_key, claim_pin) in assessment_links {
        if let Some(claim) = nodes.get_mut(&claim_pin) {
            claim.assessments.push(assessment_key);
        }
    }

    // Reconcile a private copy for scientific checks; canonical reference.status is unchanged.
    let mut derived: BTreeMap<Pin4, Value> = BTreeMap::new();
    for (k, n) in &nodes {
        let mut record = n.record.clone();
        let paths = reference_paths(&record);
        for (path, edge) in paths.iter().zip(n.edges.iter()) {
            if let Some(object) = resolve_path_mut(&mut record, path).as_object_mut() {
                object.insert("status".into(), json!(edge.status));
            }
        }
        derived.insert(k.clone(), record);
    }
    let known: HashMap<orbit_research_owner::RecordKey, Value> = derived
        .values()
        .map(|record| (orbit_research_owner::record_key(record), record.clone()))
        .collect();
    for (k, record) in &derived {
        let mut extra = Vec::new();
        if record.get("schema_version").and_then(Value::as_u64) == Some(2) {
            extra.extend(orbit_research_owner::native_reference_errors(record, &known));
        }
        if text(record, "kind") == Some("assessment")
            && record.get("payload").and_then(|payload| text(payload, "inference"))
                == Some("confirmatory-primary")
        {
            extra.extend(orbit_research_owner::confirmation_errors(
                record,
                &known,
                &HashSet::new(),
            ));
        }
        if !extra.is_empty() {
            nodes.get_mut(k).expect("derived from nodes").reasons.extend(extra);
        }
    }

    // Fixed point propagates every unresolved dependency, including cycles, without recursion.
    let mut pending: BTreeSet<Pin4> = nodes
        .iter()
        .filter(|(_, n)| !n.reasons.is_empty() || n.edges.iter().any(|edge| edge.status == "pending"))
        .map(|(k, _)| k.clone())
        .collect();
    loop {
        let mut more = BTreeSet::new();
        for (k, n) in &nodes {
            if n.edges.iter().any(|edge| pending.contains(&pin(&edge.reference))) {
                more.insert(k.clone());
            }
        }
        if more.is_subset(&pending) {
            break;
        }
        pending.extend(more);
    }
    for (k, n) in nodes.iter_mut() {
        for edge in n.edges.iter_mut() {
            if pending.contains(&pin(&edge.reference)) {
                edge.status = "pending";
                if edge.reason.is_empty() {
                    edge.reason = "target has unresolved dependencies; inspect exact trace".to_owned();
                }
            }
        }
        n.reconciliation = if pending.contains(k) { "pending" } else { "resolved" };
    }
    for (k, n) in nodes.iter_mut() {
        n.confirmation = if text(&n.record, "kind") == Some("assessment")
            && n.record.get("payload").and_then(|payload| text(payload, "inference"))
                == Some("confirmatory-primary")
            && n.history == "current"
            && text(&n.record, "activity") == Some("active")
            && !pending.contains(k)
        {
            "eligible".to_owned()
        } else {
            "not-current".to_owned()
        };
    }
    let claim_checks: Vec<(Pin4, Option<Pin4>)> = nodes
        .iter()
        .filter(|(_, n)| text(&n.record, "kind") == Some("assessment"))
        .map(|(k, n)| {
            let claim_pin = n
                .record
                .get("payload")
                .and_then(|payload| payload.get("claim"))
                .map(pin);
            (k.clone(), claim_pin)
        })
        .collect();
    for (k, claim_pin) in claim_checks {
        let Some(claim_pin) = claim_pin else { continue };
        let snapshot = nodes
            .get(&claim_pin)
            .map(|claim| (claim.history, text(&claim.record, "activity").map(str::to_owned)));
        if let Some((claim_history, claim_activity)) = snapshot
            && (claim_history != "current" || claim_activity.as_deref() != Some("active"))
        {
            let node = nodes.get_mut(&k).expect("key from nodes");
            node.confirmation = "not-current".to_owned();
            node.reasons.insert("assessment targets a historical, superseded or inactive claim".to_owned());
        }
    }

    // Competing eligible assessments do not silently adjudicate one another.
    let key_to_pin: HashMap<String, Pin4> =
        nodes.iter().map(|(k, n)| (n.key.clone(), k.clone())).collect();
    let claim_keys: Vec<Pin4> = nodes.keys().cloned().collect();
    for claim_key in &claim_keys {
        let assessment_keys = nodes.get(claim_key).map(|n| n.assessments.clone()).unwrap_or_default();
        let eligible: Vec<Pin4> = assessment_keys
            .iter()
            .filter_map(|assessment_key| key_to_pin.get(assessment_key))
            .filter(|assessment_pin| {
                nodes.get(*assessment_pin).is_some_and(|n| n.confirmation == "eligible")
            })
            .cloned()
            .collect();
        let verdicts: BTreeSet<String> = eligible
            .iter()
            .filter_map(|assessment_pin| nodes.get(assessment_pin))
            .map(|n| {
                n.record
                    .get("payload")
                    .and_then(|payload| text(payload, "verdict"))
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        if verdicts.len() > 1 {
            for assessment_pin in &eligible {
                if let Some(n) = nodes.get_mut(assessment_pin) {
                    n.confirmation = "conflicting".to_owned();
                    n.reasons.insert("current assessments disagree on this exact claim pin".to_owned());
                }
            }
        }
    }
    let mut conflicts: BTreeSet<Pin4> = nodes
        .iter()
        .filter(|(_, n)| n.confirmation == "conflicting")
        .map(|(k, _)| k.clone())
        .collect();
    while !conflicts.is_empty() {
        for k in &conflicts {
            if let Some(n) = nodes.get_mut(k) {
                n.reconciliation = "pending";
                if n.confirmation == "eligible" {
                    n.confirmation = "not-current".to_owned();
                }
            }
        }
        let mut dependents = BTreeSet::new();
        for (k, n) in nodes.iter_mut() {
            for edge in n.edges.iter_mut() {
                if conflicts.contains(&pin(&edge.reference)) {
                    edge.status = "pending";
                    edge.reason = "target has conflicting assessments in its evidence closure".to_owned();
                    if n.reconciliation != "pending" {
                        dependents.insert(k.clone());
                    }
                }
            }
        }
        conflicts = dependents;
    }

    let mut unresolved: Vec<Value> = Vec::new();
    for manifest in manifests {
        for reference in manifest
            .get("references")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !nodes.contains_key(&pin(reference)) {
                unresolved.push(json!({
                    "reference": reference,
                    "reason": "manifest target is absent",
                    "target": Value::Null,
                    "status": "pending",
                }));
            }
        }
    }
    unresolved.sort_by_key(|value| orbit_research_contract::canonical_json(value).unwrap_or_default());

    // Missing dependencies are valid pending data, not an excuse to silently reuse an old
    // resolved edge; re-check every observed byte one last time before publishing.
    for (path, digest) in observations.iter() {
        if &crate::file_digest(path)? != digest {
            return Err(IndexError::Build(vec![Problem {
                source: path.to_string_lossy().into_owned(),
                reason: "owner document changed during rebuild".to_owned(),
            }]));
        }
    }

    let mut manifests_sorted = manifests.to_vec();
    manifests_sorted.sort_by_key(|value| orbit_research_contract::canonical_json(value).unwrap_or_default());
    let records: Vec<Value> = nodes.values().map(Node::to_value).collect();
    let mut owners: Vec<String> = roots.keys().cloned().collect();
    owners.sort();
    let mut result = json!({
        "schema_version": 1,
        "kind": "research-index",
        "records": records,
        "manifests": manifests_sorted,
        "unresolved": unresolved,
        "inventories": inventories,
        "owners": owners,
        "notice": "Snapshot projection. Rebuild to observe owner changes. Execution success is not scientific support.",
    });
    let digest = crate::digest_bytes(&orbit_research_contract::canonical_json(&result)?);
    result["content_digest"] = json!(digest);
    Ok(result)
}

fn record_code(record: &Value) -> Option<&Value> {
    let payload = record.get("payload")?;
    payload
        .get("code")
        .filter(|value| !value.is_null())
        .or_else(|| payload.get("semantic").and_then(|semantic| semantic.get("code")).filter(|value| !value.is_null()))
}

// ---------------------------------------------------------------------------------------
// Source and artifact byte verification, and the native-receipt check for schema v2 records.
// ---------------------------------------------------------------------------------------

fn source_state(
    record: &Value,
    roots: &BTreeMap<String, Checkout>,
    cache: &mut HashMap<(String, String, String, String), Vec<String>>,
    observations: &mut HashMap<PathBuf, String>,
) -> Vec<String> {
    let provenance = record.get("provenance").cloned().unwrap_or(Value::Null);
    let repository = text(&provenance, "repository").unwrap_or_default().to_owned();
    let Some(root) = roots.get(&repository) else {
        return vec!["owner checkout is not mapped".to_owned()];
    };
    let git_revision = text(&provenance, "git_revision").unwrap_or_default().to_owned();
    let working_tree = provenance.get("working_tree").and_then(Value::as_bool).unwrap_or(false);
    if git_revision.is_empty() || working_tree {
        return vec!["source is unpinned or marked working-tree; owner reconciliation required".to_owned()];
    }
    let path = text(&provenance, "path").unwrap_or_default().to_owned();
    let sha256 = text(&provenance, "sha256").unwrap_or_default().to_owned();
    let identity = (repository, git_revision.clone(), path.clone(), sha256.clone());
    if !cache.contains_key(&identity) {
        let blob_oid = provenance.get("blob_oid").and_then(Value::as_str).map(str::to_owned);
        let reasons = match source_check(root, &path, &git_revision, blob_oid.as_deref(), &sha256, observations) {
            Ok(reasons) => reasons,
            Err(error) => vec![error.to_string()],
        };
        cache.insert(identity.clone(), reasons);
    }
    let mut reasons = cache.get(&identity).cloned().unwrap_or_default();
    if record.get("schema_version").and_then(Value::as_u64) == Some(2)
        && reasons.is_empty()
        && let Err(error) = verify_native_snapshot(record, root, &path, &git_revision)
    {
        reasons.push(format!("native receipt: {error}"));
    }
    reasons
}

fn source_check(
    root: &Checkout,
    path: &str,
    git_revision: &str,
    blob_oid: Option<&str>,
    sha256: &str,
    observations: &mut HashMap<PathBuf, String>,
) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    let safe = root.safe_path(path)?;
    let raw = root.git_bytes(git_revision, path)?;
    if crate::digest_bytes(&raw) != sha256 {
        reasons.push("pinned source bytes do not match provenance SHA-256".to_owned());
    }
    if let Some(blob_oid) = blob_oid
        && root.text(&["rev-parse", &format!("{git_revision}:{path}")]).as_deref() != Some(blob_oid)
    {
        reasons.push("pinned source blob differs from provenance".to_owned());
    }
    if !safe.is_file() {
        reasons.push("source missing from mapped checkout".to_owned());
    } else {
        let digest = crate::file_digest(&safe)?;
        observations.insert(safe.clone(), digest.clone());
        if digest != sha256 {
            reasons.push("source changed in mapped checkout; exact history retained".to_owned());
        }
    }
    Ok(reasons)
}

/// Re-read one committed native receipt at its exact pin and verify it matches this export
/// byte-for-byte. Mirrors the owner crate's private `Owner::snapshot`, which the index may
/// not call directly (it only depends on `owner`'s read APIs).
fn verify_native_snapshot(record: &Value, checkout: &Checkout, path: &str, revision: &str) -> Result<()> {
    checkout.safe_path(path)?;
    let data = checkout.git_bytes(revision, path)?;
    let mut committed = orbit_research_contract::parse_json(&data)?;
    require(
        orbit_research_contract::validate_structural(&committed).is_empty(),
        "invalid pinned record",
    )?;
    orbit_research_owner::NativeReceipt::new(&committed, path).verify_chain(checkout, revision)?;
    let blob_oid = checkout.text(&["rev-parse", &format!("{revision}:{path}")]);
    if let Some(object) = committed.get_mut("provenance").and_then(Value::as_object_mut) {
        object.insert("git_revision".into(), json!(revision));
        object.insert("blob_oid".into(), blob_oid.map_or(Value::Null, Value::from));
        object.insert("sha256".into(), json!(crate::digest_bytes(&data)));
        object.insert("path".into(), json!(path));
        object.insert("selector".into(), json!("$"));
        object.insert("working_tree".into(), json!(false));
    }
    require(
        orbit_research_contract::canonical_json(&committed)?
            == orbit_research_contract::canonical_json(record)?,
        "native export differs from exact owner receipt",
    )
}

fn artifact_state(
    record: &Value,
    roots: &BTreeMap<String, Checkout>,
    observations: &mut HashMap<PathBuf, String>,
) -> Vec<String> {
    let Some(payload) = record.get("payload") else {
        return vec![];
    };
    let availability = text(payload, "availability").unwrap_or_default();
    if availability != "available" {
        return vec![format!("artifact is {availability}")];
    }
    let locator = text(payload, "locator");
    let repository = record.get("provenance").and_then(|p| text(p, "repository"));
    let root = repository.and_then(|repository| roots.get(repository));
    let (Some(root), Some(locator)) = (root, locator) else {
        return vec!["artifact bytes are not locally mapped".to_owned()];
    };
    // Remote URLs and opaque locators stay explicit missingness, never downloads.
    if locator.contains(':') || locator.contains('\\') {
        return vec!["artifact locator requires explicit owner verification; no automatic fetch".to_owned()];
    }
    let path = match root.safe_path(locator) {
        Ok(path) => path,
        Err(error) => return vec![error.to_string()],
    };
    if !path.is_file() {
        return vec!["artifact bytes missing from mapped checkout".to_owned()];
    }
    let digest = match crate::file_digest(&path) {
        Ok(digest) => digest,
        Err(error) => return vec![error.to_string()],
    };
    observations.insert(path, digest.clone());
    let expected = text(payload, "snapshot_digest").unwrap_or_default();
    if digest != expected {
        return vec!["artifact bytes differ from snapshot digest (or require a typed owner verifier)".to_owned()];
    }
    vec![]
}

// ---------------------------------------------------------------------------------------
// Mutable traversal of exactly the reference-shaped locations `record_references` reads,
// in the same order, so private per-record copies can carry the index's own reconciled
// pending/resolved status without ever touching the record read from an owner document.
// ---------------------------------------------------------------------------------------

enum RefPath {
    References(usize),
    Supersedes(usize),
    Claim,
    Evidence(usize),
    Protocol,
    ResultArtifacts(usize),
    Inputs(usize),
    Start,
    FreezeEvidence,
    SemanticClaims(usize),
    SemanticInputs(usize),
}

fn reference_paths(record: &Value) -> Vec<RefPath> {
    let mut paths = Vec::new();
    if let Some(values) = record.get("references").and_then(Value::as_array) {
        paths.extend((0..values.len()).map(RefPath::References));
    }
    if let Some(values) = record
        .get("authorship")
        .and_then(|authorship| authorship.get("supersedes"))
        .and_then(Value::as_array)
    {
        paths.extend((0..values.len()).map(RefPath::Supersedes));
    }
    let Some(payload) = record.get("payload") else {
        return paths;
    };
    match text(record, "kind") {
        Some("assessment") => {
            if payload.get("claim").is_some() {
                paths.push(RefPath::Claim);
            }
            if let Some(values) = payload.get("evidence").and_then(Value::as_array) {
                paths.extend((0..values.len()).map(RefPath::Evidence));
            }
        }
        Some("experiment") => {
            if payload.get("protocol").is_some_and(|value| !value.is_null()) {
                paths.push(RefPath::Protocol);
            }
            if let Some(values) = payload.get("result_artifacts").and_then(Value::as_array) {
                paths.extend((0..values.len()).map(RefPath::ResultArtifacts));
            }
            if let Some(values) = payload.get("inputs").and_then(Value::as_array) {
                paths.extend((0..values.len()).map(RefPath::Inputs));
            }
            if payload.get("start").is_some_and(|value| !value.is_null()) {
                paths.push(RefPath::Start);
            }
        }
        Some("protocol") => {
            if payload.get("freeze_evidence").is_some_and(|value| !value.is_null()) {
                paths.push(RefPath::FreezeEvidence);
            }
            if record.get("schema_version").and_then(Value::as_u64) == Some(2)
                && let Some(semantic) = payload.get("semantic")
            {
                if let Some(values) = semantic.get("claims").and_then(Value::as_array) {
                    paths.extend((0..values.len()).map(RefPath::SemanticClaims));
                }
                if let Some(values) = semantic.get("inputs").and_then(Value::as_array) {
                    paths.extend((0..values.len()).map(RefPath::SemanticInputs));
                }
            }
        }
        _ => {}
    }
    paths
}

fn resolve_path_mut<'a>(record: &'a mut Value, path: &RefPath) -> &'a mut Value {
    match *path {
        RefPath::References(i) => &mut record["references"][i],
        RefPath::Supersedes(i) => &mut record["authorship"]["supersedes"][i],
        RefPath::Claim => &mut record["payload"]["claim"],
        RefPath::Evidence(i) => &mut record["payload"]["evidence"][i],
        RefPath::Protocol => &mut record["payload"]["protocol"],
        RefPath::ResultArtifacts(i) => &mut record["payload"]["result_artifacts"][i],
        RefPath::Inputs(i) => &mut record["payload"]["inputs"][i],
        RefPath::Start => &mut record["payload"]["start"],
        RefPath::FreezeEvidence => &mut record["payload"]["freeze_evidence"],
        RefPath::SemanticClaims(i) => &mut record["payload"]["semantic"]["claims"][i],
        RefPath::SemanticInputs(i) => &mut record["payload"]["semantic"]["inputs"][i],
    }
}
