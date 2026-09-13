//! Competing confirmatory-primary assessments must not silently confirm one exact claim.
//! Builds a full native authoring chain (program, claim, frozen protocol, two independent
//! completed-and-passed runs) through the real owner crate, then exports and indexes it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_research_owner::{Owner, OwnerConfig, reference};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// A real file inside the checkout's working tree, so the index's own local artifact-byte
/// verification (never a colon-opaque or remote locator) resolves it instead of leaving it
/// pending, which would otherwise break the deeper confirmatory evidence-closure check.
fn write_local_artifact(root: &Path, relative: &str, content: &[u8]) -> String {
    let full = root.join(relative);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("artifact directory");
    std::fs::write(&full, content).expect("write artifact");
    format!("sha256:{:x}", Sha256::digest(content))
}

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

const LINK: fn() -> Value =
    || json!({"host": "fixture-host", "workspace": "ws_fixture", "task": "ORB-fixture", "run": "jrun-fixture"});

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

fn initialize(root: &Path) {
    std::fs::create_dir_all(root).expect("owner root");
    git(root, &["init", "-q"]);
    std::fs::write(root.join("code.py"), "# Fixture apparatus only; no experiment is run.\n")
        .expect("fixture file");
    git(root, &["add", "code.py"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture apparatus",
        ],
    );
}

fn commit(root: &Path) -> String {
    git(root, &["add", "research/records"]);
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
    git(root, &["rev-parse", "HEAD"])
}

fn owner(root: &Path) -> Owner {
    Owner::open(root, "fixture", OwnerConfig::default()).expect("owner opens")
}

fn request(ident: &str, payload: Value) -> Value {
    json!({
        "request_id": ident,
        "id": ident,
        "payload": payload,
        "scope": "synthetic-calibration",
        "orbit_links": [LINK()],
        "expected_heads": [],
        "reason": "Synthetic acceptance fixture.",
    })
}

fn with(request: &Value, fields: &[(&str, Value)]) -> Value {
    let mut updated = request.clone();
    let object = updated.as_object_mut().expect("request object");
    for (key, value) in fields {
        object.insert((*key).to_owned(), value.clone());
    }
    updated
}

fn claim_payload(statement: &str) -> Value {
    json!({"role": "claim", "statement": statement, "domain": "model"})
}

fn dataset_payload(digest: &str, locator: &str) -> Value {
    json!({
        "role": "dataset",
        "availability": "available",
        "snapshot_digest": digest,
        "locator": locator,
        "media_type": "application/json",
    })
}

fn semantic(claim: &Value, data: &Value, code: &Value, boundary: &str, digest: &str) -> Value {
    json!({
        "question": "Does the fixture satisfy this exact model property?",
        "assumptions": "Synthetic apparatus only; no claim about nature.",
        "analysis": "Fixed declared estimator.",
        "exclusions": "None.",
        "stopping_rule": "Fixed budget of 100 enumerated fixtures.",
        "claims": [claim],
        "inputs": [data],
        "code": code,
        "design": {
            "kind": "synthetic",
            "samples": {"total": 100, "groups": [{"name": "null", "count": 50}, {"name": "signal", "count": 50}]},
            "baseline": "null estimator",
            "controls": ["negative", "estimator"],
            "decision": {"metric": "detection rate", "operator": ">=", "threshold": 0.9, "attainable_min": 0, "attainable_max": 1},
            "resource_budget": {"planned": 100, "limit": 200, "unit": "evaluations"},
        },
        "holdout": {
            "digest": digest,
            "information_cutoff": "2026-01-01T00:00:00+00:00",
            "evaluation_not_before": boundary,
            "policy": "Commit the seed plan before generating fixture evaluations.",
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn run_payload(
    protocol: &Value,
    data: &Value,
    code: &Value,
    status: &str,
    controls: &str,
    start: Option<&Value>,
    results: &[Value],
    digest: &str,
) -> Value {
    json!({
        "execution_status": status,
        "controls": controls,
        "control_results": {"negative": controls, "estimator": controls},
        "protocol": protocol,
        "result_artifacts": results,
        "inputs": [data],
        "code": code,
        "environment": {"fixture": true},
        "invocation": ["python", "code.py"],
        "deviations": [],
        "start": start.cloned().unwrap_or(Value::Null),
        "holdout_digest": digest,
    })
}

fn pinned(owner: &Owner, root: &Path, record: &Value) -> Value {
    let head = commit(root);
    let snapshot = owner
        .pin(
            record["id"].as_str().expect("id"),
            record["revision_id"].as_str().expect("revision"),
            &head,
        )
        .expect("exact pin");
    reference(&snapshot, "resolved")
}

fn boundary_in(delay: Duration) -> String {
    let moment = SystemTime::now() + delay;
    let elapsed = moment.duration_since(UNIX_EPOCH).expect("after the epoch");
    let seconds = i64::try_from(elapsed.as_secs()).unwrap_or_default();
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}+00:00",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60,
        elapsed.subsec_micros()
    )
}

fn civil(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 }).unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// A second independent completed, passed-controls run, so a second confirmatory-primary
/// assessment can be `eligible` in its own right before the index checks whether two
/// disagreeing eligible assessments of the same claim get silently reconciled.
#[allow(clippy::too_many_arguments)]
fn completed_run(
    owner: &Owner,
    root: &Path,
    ident: &str,
    protocol_ref: &Value,
    data_ref: &Value,
    code: &Value,
    seed_digest: &str,
    output_ident: &str,
) -> Value {
    let start = owner
        .apply(
            "begin-run",
            &request(
                ident,
                run_payload(protocol_ref, data_ref, code, "running", "not-run", None, &[], seed_digest),
            ),
        )
        .expect("run start");
    let start_ref = pinned(owner, root, &start);

    let locator = format!("results/{output_ident}.txt");
    let result_digest = write_local_artifact(
        root,
        &locator,
        format!("Fixture result for {output_ident}; no scientific record.\n").as_bytes(),
    );
    let output = owner
        .apply(
            "artifact",
            &request(
                output_ident,
                json!({
                    "role": "result",
                    "availability": "available",
                    "snapshot_digest": result_digest,
                    "locator": locator,
                    "media_type": "text/plain",
                }),
            ),
        )
        .expect("result artifact");
    let output_ref = pinned(owner, root, &output);

    let completed = owner
        .apply(
            "record-run",
            &with(
                &request(
                    ident,
                    run_payload(
                        protocol_ref,
                        data_ref,
                        code,
                        "completed",
                        "passed",
                        Some(&start_ref),
                        std::slice::from_ref(&output_ref),
                        seed_digest,
                    ),
                ),
                &[
                    ("request_id", json!(format!("{ident}-finish"))),
                    ("expected_heads", json!([start["revision_id"]])),
                ],
            ),
        )
        .expect("completed run");
    pinned(owner, root, &completed)
}

#[test]
fn competing_eligible_assessments_do_not_silently_confirm() {
    let scratch = Scratch::new("confirmation");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);
    let code = json!({"repository": "fixture", "git_revision": git(&root, &["rev-parse", "HEAD"])});
    let seed_digest = write_local_artifact(&root, "data/seed-plan.bin", b"Fixture seed plan; no scientific record.\n");

    let program = owner
        .apply(
            "program",
            &request(
                "control-program",
                json!({"role": "program", "title": "Confirmation fixture", "question": "Does the estimator pass?"}),
            ),
        )
        .expect("program");
    let program_ref = pinned(&owner, &root, &program);

    let claim = owner
        .apply(
            "claim",
            &with(
                &request("C1", claim_payload("The synthetic estimator passes its declared controls.")),
                &[("references", json!([program_ref]))],
            ),
        )
        .expect("claim");
    let claim_ref = pinned(&owner, &root, &claim);

    let data = owner
        .apply(
            "artifact",
            &request("seed-plan", dataset_payload(&seed_digest, "data/seed-plan.bin")),
        )
        .expect("dataset");
    let data_ref = pinned(&owner, &root, &data);

    let boundary = boundary_in(Duration::from_millis(600));
    let protocol = owner
        .apply(
            "preregister",
            &request("P1", json!({"semantic": semantic(&claim_ref, &data_ref, &code, &boundary, &seed_digest)})),
        )
        .expect("protocol");
    let protocol_ref = pinned(&owner, &root, &protocol);

    std::thread::sleep(Duration::from_millis(700));

    let completed_ref_1 = completed_run(
        &owner, &root, "E1", &protocol_ref, &data_ref, &code, &seed_digest, "fixture-output-1",
    );
    let completed_ref_2 = completed_run(
        &owner, &root, "E2", &protocol_ref, &data_ref, &code, &seed_digest, "fixture-output-2",
    );

    let supported = owner
        .apply(
            "assess",
            &request(
                "assessment-supported",
                json!({
                    "claim": claim_ref,
                    "verdict": "supported",
                    "inference": "confirmatory-primary",
                    "controls": "passed",
                    "basis": "scientific-evidence",
                    "rationale": "First run meets the fixed decision rule.",
                    "evidence": [completed_ref_1],
                    "legacy_verdict": Value::Null,
                    "evidence_summary": "supports",
                }),
            ),
        )
        .expect("first primary confirmation");
    pinned(&owner, &root, &supported);

    let refuted = owner
        .apply(
            "assess",
            &request(
                "assessment-refuted",
                json!({
                    "claim": claim_ref,
                    "verdict": "refuted",
                    "inference": "confirmatory-primary",
                    "controls": "passed",
                    "basis": "scientific-evidence",
                    "rationale": "Second independent run disagrees with the first.",
                    "evidence": [completed_ref_2],
                    "legacy_verdict": Value::Null,
                    "evidence_summary": "refutes",
                }),
            ),
        )
        .expect("second primary confirmation");
    pinned(&owner, &root, &refuted);

    let head = git(&root, &["rev-parse", "HEAD"]);
    let bundle = owner.export(&head).expect("export");

    // The export re-pins every record at the export's own head, not at the commit each was
    // originally appended at; look these two assessments up the same way for a match.
    let supported_reference = reference(
        &owner
            .pin(supported["id"].as_str().expect("id"), supported["revision_id"].as_str().expect("revision"), &head)
            .expect("pin supported assessment at export head"),
        "resolved",
    );
    let refuted_reference = reference(
        &owner
            .pin(refuted["id"].as_str().expect("id"), refuted["revision_id"].as_str().expect("revision"), &head)
            .expect("pin refuted assessment at export head"),
        "resolved",
    );

    let documents = scratch.join("documents");
    std::fs::create_dir_all(&documents).expect("documents directory");
    std::fs::write(documents.join("export.json"), serde_json::to_vec(&bundle).expect("bytes"))
        .expect("write export bundle");

    let config = json!({
        "schema_version": 1,
        "checkouts": {"fixture": root.to_string_lossy()},
        "documents": [documents.to_string_lossy()],
    });
    let config_path = scratch.join("index-config.json");
    std::fs::write(&config_path, serde_json::to_vec(&config).expect("bytes")).expect("write config");

    let database = scratch.join("index.sqlite");
    orbit_research_index::rebuild(&config_path, &database).expect("rebuild succeeds");

    let projection = orbit_research_index::read_index(&database).expect("read index");
    let records = projection["records"].as_array().expect("records");
    let node_for = |reference: &Value| -> &Value {
        let pin = [
            reference["repository"].clone(),
            reference["id"].clone(),
            reference["revision_id"].clone(),
            reference["source_revision"].clone(),
        ];
        records
            .iter()
            .find(|node| node["pin"] == json!(pin))
            .unwrap_or_else(|| panic!("missing node for {reference}"))
    };

    let supported_node = node_for(&supported_reference);
    let refuted_node = node_for(&refuted_reference);
    assert_eq!(
        "conflicting", supported_node["confirmation"],
        "a disagreeing eligible assessment must not leave the first one silently confirmed"
    );
    assert_eq!(
        "conflicting", refuted_node["confirmation"],
        "a disagreeing eligible assessment must not silently confirm itself either"
    );
    assert_eq!("pending", supported_node["reconciliation"]);
    assert_eq!("pending", refuted_node["reconciliation"]);
}
