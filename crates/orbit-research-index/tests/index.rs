//! Behavioural coverage for the disposable projection, against real Git fixtures: atomic
//! rebuild, an invalid rebuild retaining the previous database, and pending vs. resolved
//! pins. Competing confirmatory-primary assessments are covered separately in
//! `native_confirmation.rs`, since that scenario needs a full native authoring chain.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "orbit-research-index-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn init_checkout(root: &Path) {
    std::fs::create_dir_all(root).expect("checkout root");
    git(root, &["init", "-q"]);
    std::fs::write(root.join(".keep"), b"Fixture checkout; no scientific record.\n").expect("seed file");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture checkout",
        ],
    );
}

fn digest_bytes(data: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(data))
}

/// One v1 record with a placeholder provenance and a freshly computed `revision_id`, ready
/// for `commit_records` to pin at an exact commit.
fn build_record(repository: &str, kind: &str, slug: &str, payload: Value, references: Vec<Value>) -> Value {
    let placeholder_sha256 = format!("sha256:{}", "0".repeat(64));
    let mut record = json!({
        "schema_version": 1,
        "kind": kind,
        "id": format!("urn:research:{repository}:{kind}:{slug}"),
        "aliases": [],
        "activity": "active",
        "scope": "synthetic-calibration",
        "provenance": {
            "repository": repository,
            "git_revision": Value::Null,
            "blob_oid": Value::Null,
            "sha256": placeholder_sha256,
            "path": format!("research/records/{slug}.json"),
            "selector": "$",
            "historical": false,
            "working_tree": true,
        },
        "limitations": [],
        "missingness": [],
        "legacy": Value::Null,
        "references": references,
        "presentation": {},
        "payload": payload,
    });
    record.as_object_mut().expect("object").remove("revision_id");
    let digest = orbit_research_contract::revision_digest(&record).expect("revision digest");
    record["revision_id"] = json!(digest);
    record
}

/// Write, commit and pin every supplied record at one exact shared commit.
fn commit_records(root: &Path, records: &mut [Value]) {
    let mut bytes_by_slot = Vec::with_capacity(records.len());
    for record in records.iter() {
        let relative = record["provenance"]["path"].as_str().expect("path").to_owned();
        let bytes = orbit_research_contract::canonical_json(record).expect("canonical json");
        let full = root.join(&relative);
        std::fs::create_dir_all(full.parent().expect("parent")).expect("record directory");
        std::fs::write(&full, &bytes).expect("write record");
        bytes_by_slot.push(bytes);
    }
    git(root, &["add", "research"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "immutable fixture records",
        ],
    );
    let head = git(root, &["rev-parse", "HEAD"]);
    for (record, bytes) in records.iter_mut().zip(bytes_by_slot.iter()) {
        let repository = record["provenance"]["repository"].clone();
        let path = record["provenance"]["path"].clone();
        record["provenance"] = json!({
            "repository": repository,
            "git_revision": head,
            "blob_oid": Value::Null,
            "sha256": digest_bytes(bytes),
            "path": path,
            "selector": "$",
            "historical": false,
            "working_tree": false,
        });
    }
}

fn reference_to(record: &Value, status: &str) -> Value {
    json!({
        "repository": record["provenance"]["repository"],
        "id": record["id"],
        "revision_id": record["revision_id"],
        "source_revision": record["provenance"]["git_revision"],
        "status": status,
    })
}

fn manifest_for(repository: &str, git_revision: &str, references: &[Value]) -> Value {
    json!({
        "schema_version": 1,
        "kind": "manifest",
        "repositories": [{"id": repository, "git_revision": git_revision}],
        "references": references,
    })
}

fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("documents directory");
    std::fs::write(path, serde_json::to_vec(value).expect("bytes")).expect("write document");
}

fn config_for(scratch: &Scratch, checkout_root: &Path, documents_dir: &Path) {
    let config = json!({
        "schema_version": 1,
        "checkouts": {"fixture": checkout_root.to_string_lossy()},
        "documents": [documents_dir.to_string_lossy()],
    });
    write_json(&scratch.join("index-config.json"), &config);
}

#[test]
fn atomic_rebuild_and_invalid_rebuild_retains_previous_database() {
    let scratch = Scratch::new("atomic");
    let checkout = scratch.join("owner");
    init_checkout(&checkout);

    let mut program = build_record(
        "fixture",
        "program",
        "P1",
        json!({"role": "program", "title": "Atomic rebuild fixture"}),
        vec![],
    );
    commit_records(&checkout, std::slice::from_mut(&mut program));
    let head = program["provenance"]["git_revision"].as_str().expect("head").to_owned();

    let documents = scratch.join("documents");
    write_json(&documents.join("program.json"), &program);
    write_json(
        &documents.join("manifest.json"),
        &manifest_for("fixture", &head, &[reference_to(&program, "resolved")]),
    );
    config_for(&scratch, &checkout, &documents);

    let config_path = scratch.join("index-config.json");
    let database = scratch.join("index.sqlite");

    let outcome = orbit_research_index::rebuild(&config_path, &database).expect("first rebuild succeeds");
    assert_eq!(1, outcome.records);
    assert_eq!(0, outcome.pending);

    let published = orbit_research_index::read_index(&database).expect("read published index");
    assert_eq!(outcome.content_digest, published["content_digest"]);

    // Corrupt one document: fail-closed, and never touch the previously published database.
    std::fs::write(documents.join("program.json"), b"{not json").expect("corrupt document");
    let error = orbit_research_index::rebuild(&config_path, &database).expect_err("invalid rebuild is refused");
    assert!(error.problems().is_some_and(|problems| !problems.is_empty()), "{error}");

    let still_published = orbit_research_index::read_index(&database).expect("previous index remains readable");
    assert_eq!(published, still_published, "invalid rebuild must not touch the previous database");
}

#[test]
fn pending_vs_resolved_pins() {
    let scratch = Scratch::new("pending");
    let checkout = scratch.join("owner");
    init_checkout(&checkout);

    let mut program = build_record(
        "fixture",
        "program",
        "P1",
        json!({"role": "program", "title": "Pending vs resolved fixture"}),
        vec![],
    );
    commit_records(&checkout, std::slice::from_mut(&mut program));
    let program_reference = reference_to(&program, "resolved");

    let dangling_reference = json!({
        "repository": "fixture",
        "id": "urn:research:fixture:claim:absent",
        "revision_id": format!("sha256:{}", "a".repeat(64)),
        "source_revision": program["provenance"]["git_revision"],
        "status": "resolved",
    });

    let resolved_claim = build_record(
        "fixture",
        "claim",
        "resolved-claim",
        json!({"role": "claim", "statement": "The program exists.", "domain": "model"}),
        vec![program_reference.clone()],
    );
    let pending_claim = build_record(
        "fixture",
        "claim",
        "pending-claim",
        json!({"role": "claim", "statement": "Something unrecorded is true.", "domain": "model"}),
        vec![dangling_reference],
    );
    let mut claims = vec![resolved_claim, pending_claim];
    commit_records(&checkout, &mut claims);
    let pending_claim = claims.pop().expect("pending claim");
    let resolved_claim = claims.pop().expect("resolved claim");
    let head = resolved_claim["provenance"]["git_revision"]
        .as_str()
        .expect("head")
        .to_owned();

    let program_head = program["provenance"]["git_revision"].as_str().expect("head").to_owned();
    let documents = scratch.join("documents");
    write_json(&documents.join("program.json"), &program);
    write_json(&documents.join("resolved-claim.json"), &resolved_claim);
    write_json(&documents.join("pending-claim.json"), &pending_claim);
    // Each record's manifest pin must match the exact commit it was itself published at;
    // program and the two claims are committed separately, so they need separate manifests.
    write_json(
        &documents.join("manifest-program.json"),
        &manifest_for("fixture", &program_head, &[program_reference]),
    );
    write_json(
        &documents.join("manifest-claims.json"),
        &manifest_for(
            "fixture",
            &head,
            &[
                reference_to(&resolved_claim, "resolved"),
                reference_to(&pending_claim, "resolved"),
            ],
        ),
    );
    config_for(&scratch, &checkout, &documents);

    let config_path = scratch.join("index-config.json");
    let database = scratch.join("index.sqlite");
    let outcome = orbit_research_index::rebuild(&config_path, &database).expect("rebuild succeeds");
    assert_eq!(3, outcome.records);
    assert_eq!(1, outcome.pending, "only the dangling claim is pending");

    let projection = orbit_research_index::read_index(&database).expect("read index");
    let records = projection["records"].as_array().expect("records");
    let node = |id: &str| -> &Value {
        records
            .iter()
            .find(|node| node["record"]["id"] == id)
            .unwrap_or_else(|| panic!("missing node for {id}"))
    };

    let resolved_node = node(resolved_claim["id"].as_str().expect("id"));
    assert_eq!("resolved", resolved_node["reconciliation"]);
    let resolved_edges = resolved_node["edges"].as_array().expect("edges");
    assert_eq!(1, resolved_edges.len());
    assert_eq!("resolved", resolved_edges[0]["status"]);

    let pending_node = node(pending_claim["id"].as_str().expect("id"));
    assert_eq!("pending", pending_node["reconciliation"]);
    let pending_edges = pending_node["edges"].as_array().expect("edges");
    assert_eq!(1, pending_edges.len());
    assert_eq!("pending", pending_edges[0]["status"]);
    assert_eq!("exact target is absent", pending_edges[0]["reason"]);

    // `index-trace` follows exact dependency pins, including unresolved ones.
    let trace = orbit_research_index::trace(&database, pending_node["key"].as_str().expect("key"))
        .expect("trace resolves");
    assert_eq!(1, trace["records"].as_array().expect("records").len());
}
