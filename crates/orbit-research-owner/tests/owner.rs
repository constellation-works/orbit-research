//! Behavioural tests for the native write path, against real Git fixtures.
//!
//! Every fixture is a fresh disposable checkout outside the workspace; no scientific result
//! is produced here and no sibling owner is read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_research_contract::{revision_digest, validate};
use orbit_research_owner::{
    Checkout, FixedClock, Owner, OwnerConfig, SubprocessGit, reference, validate_native,
};
use serde_json::{Value, json};

const LINK: fn() -> Value = || json!({"host": "fixture-host", "workspace": "ws_fixture", "task": "ORB-fixture", "run": "jrun-fixture"});

/// A disposable directory outside the repository worktree.
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
            "orbit-research-owner-{label}-{}-{nanos}",
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

fn initialize(root: &Path) {
    std::fs::create_dir_all(root).expect("owner root");
    git(root, &["init", "-q"]);
    std::fs::write(
        root.join("code.py"),
        "# Fixture apparatus only; no experiment is run.\n",
    )
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

fn owner_at(root: &Path, moment: &str) -> Owner {
    Owner::open(
        root,
        "fixture",
        OwnerConfig {
            clock: Arc::new(FixedClock(moment.to_owned())),
            ..OwnerConfig::default()
        },
    )
    .expect("owner opens")
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

fn dataset_payload(digest: &str) -> Value {
    json!({
        "role": "dataset",
        "availability": "available",
        "snapshot_digest": digest,
        "locator": "fixture:seed-plan",
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

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[test]
fn appends_form_a_sequenced_tamper_evident_chain() {
    let scratch = Scratch::new("chain");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);

    let first = owner
        .apply("claim", &request("C1", claim_payload("Original.")))
        .expect("first append");
    let second = owner
        .apply("claim", &request("C2", claim_payload("Another identity.")))
        .expect("second append");

    let entries = owner.entries().expect("entries");
    assert_eq!(2, entries.len());
    assert_eq!(1, first["authorship"]["sequence"]);
    assert!(first["authorship"]["previous"].is_null());
    assert_eq!(2, second["authorship"]["sequence"]);
    assert!(second["authorship"]["previous"].is_string());
    let (path, _) = &entries[0];
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("name");
    assert!(
        name.starts_with("00000001-") && name.ends_with(".json"),
        "{name}"
    );

    // A changed byte in a published append breaks the filename/content binding.
    let mut tampered: Value =
        serde_json::from_slice(&std::fs::read(path).expect("read")).expect("json");
    tampered["presentation"] = json!({"note": "tampered"});
    std::fs::write(path, serde_json::to_vec(&tampered).expect("bytes")).expect("write");
    let error = message(owner.records().expect_err("tampering is refused"));
    assert!(error.contains("filename/content"), "{error}");
}

#[test]
fn identical_requests_are_idempotent_and_key_reuse_fails() {
    let scratch = Scratch::new("idempotent");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);
    let original = request("C1", claim_payload("Original."));

    let first = owner.apply("claim", &original).expect("first append");
    let repeat = owner
        .apply("claim", &original)
        .expect("retry is idempotent");
    assert_eq!(first, repeat);
    assert_eq!(1, owner.records().expect("records").len());

    let changed = with(&original, &[("payload", claim_payload("Changed."))]);
    let error = message(owner.apply("claim", &changed).expect_err("key reuse fails"));
    assert!(error.contains("idempotency"), "{error}");
    assert_eq!(1, owner.records().expect("records").len());
}

#[test]
fn stale_expected_heads_are_refused() {
    let scratch = Scratch::new("heads");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);
    let first = owner
        .apply("claim", &request("C1", claim_payload("Original.")))
        .expect("first append");

    let stale = with(
        &request("C1", claim_payload("Changed.")),
        &[("request_id", json!("revision-2"))],
    );
    let error = message(owner.apply("claim", &stale).expect_err("stale base"));
    assert!(error.contains("stale base"), "{error}");

    let current = with(&stale, &[("expected_heads", json!([first["revision_id"]]))]);
    let second = owner.apply("claim", &current).expect("current base");
    assert_eq!(
        vec![second["revision_id"].as_str().expect("revision")],
        owner
            .heads(first["id"].as_str().expect("id"))
            .expect("heads")
    );
    assert_eq!(
        first["revision_id"],
        second["authorship"]["supersedes"][0]["revision_id"]
    );

    // An explicit empty supersedes retains the conflicting branch as a second head.
    let fork = with(
        &request("C1", claim_payload("Disputed alternative.")),
        &[
            ("request_id", json!("fork")),
            ("expected_heads", json!([second["revision_id"]])),
            ("supersedes", json!([])),
        ],
    );
    let third = owner.apply("claim", &fork).expect("explicit fork");
    let mut expected = vec![
        second["revision_id"].as_str().expect("revision").to_owned(),
        third["revision_id"].as_str().expect("revision").to_owned(),
    ];
    expected.sort();
    assert_eq!(
        expected,
        owner
            .heads(first["id"].as_str().expect("id"))
            .expect("heads")
    );
}

#[test]
fn symlinked_and_escaping_record_paths_are_refused() {
    let scratch = Scratch::new("paths");
    let root = scratch.join("owner");
    initialize(&root);
    let outside = scratch.join("outside");
    std::fs::create_dir_all(&outside).expect("outside directory");
    std::os::unix::fs::symlink(&outside, root.join("research")).expect("symlink");

    let error = match Owner::open(&root, "fixture", OwnerConfig::default()) {
        Err(error) => message(error),
        Ok(_) => panic!("a symlinked records directory must be refused"),
    };
    assert!(error.contains("symlinks"), "{error}");

    let escaping = Owner::open(
        &root,
        "fixture",
        OwnerConfig {
            records: "../outside".to_owned(),
            ..OwnerConfig::default()
        },
    );
    assert!(
        escaping.is_err(),
        "records outside research/ must be refused"
    );

    // A record file that is itself a symlink is not canonical content.
    std::fs::remove_file(root.join("research")).expect("remove symlink");
    let owner = owner(&root);
    owner
        .apply("claim", &request("C1", claim_payload("Original.")))
        .expect("append");
    std::os::unix::fs::symlink(
        root.join("code.py"),
        owner.directory().join("00000002-symlink.json"),
    )
    .expect("symlink");
    let error = message(owner.records().expect_err("symlinked record"));
    assert!(error.contains("symlink"), "{error}");

    // A subdirectory is not a checkout root.
    std::fs::create_dir_all(root.join("nested")).expect("nested");
    assert!(Owner::open(&root.join("nested"), "fixture", OwnerConfig::default()).is_err());
}

#[test]
fn freeze_chronology_is_enforced_in_both_directions() {
    let scratch = Scratch::new("freeze");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);
    let code = json!({"repository": "fixture", "git_revision": git(&root, &["rev-parse", "HEAD"])});
    let digest = format!("sha256:{}", "d".repeat(64));

    let claim = owner
        .apply(
            "claim",
            &request("C1", claim_payload("A fixture property.")),
        )
        .expect("claim");
    let claim_ref = pinned(&owner, &root, &claim);
    let data = owner
        .apply("artifact", &request("D1", dataset_payload(&digest)))
        .expect("dataset");
    let data_ref = pinned(&owner, &root, &data);

    // A caller may not supply freeze metadata at all.
    let supplied = request(
        "P0",
        json!({"semantic": semantic(&claim_ref, &data_ref, &code, "2030-01-01T00:00:00+00:00", &digest), "frozen_at": "2020-01-01"}),
    );
    let error = message(
        owner
            .apply("preregister", &supplied)
            .expect_err("caller freeze"),
    );
    assert!(error.contains("caller freeze"), "{error}");

    // A boundary already in the past cannot be registered as a prospective freeze.
    let stale = request(
        "P0",
        json!({"semantic": semantic(&claim_ref, &data_ref, &code, "2026-01-02T00:00:00+00:00", &digest)}),
    );
    let error = message(
        owner
            .apply("preregister", &stale)
            .expect_err("stale boundary"),
    );
    assert!(
        error.contains("freeze must precede the evaluation boundary"),
        "{error}"
    );

    let boundary = boundary_in(Duration::from_millis(600));
    let protocol = owner
        .apply(
            "preregister",
            &request(
                "P1",
                json!({"semantic": semantic(&claim_ref, &data_ref, &code, &boundary, &digest)}),
            ),
        )
        .expect("registered protocol");
    assert_eq!("registered", protocol["payload"]["freeze"]);
    assert_eq!(
        protocol["authorship"]["registered_at"],
        protocol["payload"]["frozen_at"]
    );
    let protocol_ref = pinned(&owner, &root, &protocol);

    // A run started before the frozen evaluation boundary is refused.
    let frozen_at = protocol["payload"]["frozen_at"]
        .as_str()
        .expect("freeze time")
        .to_owned();
    let early = owner_at(&root, &frozen_at);
    let start_request = request(
        "E1",
        run_payload(
            &protocol_ref,
            &data_ref,
            &code,
            "running",
            "not-run",
            None,
            &[],
            &digest,
        ),
    );
    let error = message(
        early
            .apply("begin-run", &start_request)
            .expect_err("evaluation boundary"),
    );
    assert!(error.contains("boundary"), "{error}");

    std::thread::sleep(Duration::from_millis(700));
    let start = owner
        .apply("begin-run", &start_request)
        .expect("run start after the boundary");
    assert_eq!(
        start["authorship"]["registered_at"],
        start["payload"]["started_at"]
    );
}

#[test]
fn resolve_requires_an_exact_pin_and_never_falls_back_to_head() {
    let scratch = Scratch::new("pins");
    let root = scratch.join("owner");
    let sibling = scratch.join("sibling");
    initialize(&root);
    initialize(&sibling);
    let other = Owner::open(&sibling, "sibling", OwnerConfig::default()).expect("sibling owner");
    let program = other
        .apply(
            "program",
            &request(
                "R1",
                json!({"role": "program", "title": "Sibling program", "question": "Exact sibling question?"}),
            ),
        )
        .expect("sibling program");
    let program_ref = {
        let head = commit(&sibling);
        reference(
            &other
                .pin(
                    program["id"].as_str().expect("id"),
                    program["revision_id"].as_str().expect("revision"),
                    &head,
                )
                .expect("sibling pin"),
            "resolved",
        )
    };

    // An unrouted repository is never searched, however reachable it is on disk.
    let unrouted = owner(&root);
    let referring = with(
        &request("C1", claim_payload("Cross-owner claim.")),
        &[("references", json!([program_ref]))],
    );
    let error = message(unrouted.apply("claim", &referring).expect_err("unrouted"));
    assert!(error.contains("unrouted"), "{error}");

    let mut sources = BTreeMap::new();
    sources.insert("sibling".to_owned(), sibling.clone());
    let routed = Owner::open(
        &root,
        "fixture",
        OwnerConfig {
            sources,
            ..OwnerConfig::default()
        },
    )
    .expect("routed owner");
    routed.apply("claim", &referring).expect("routed claim");

    // A wrong publication pin fails closed instead of resolving the newest revision.
    let wrong = with(
        &referring,
        &[
            ("request_id", json!("wrong-pin")),
            ("id", json!("C2")),
            (
                "references",
                json!([{
                    "repository": "sibling",
                    "id": program["id"],
                    "revision_id": program["revision_id"],
                    "source_revision": "f".repeat(40),
                    "status": "resolved",
                }]),
            ),
        ],
    );
    assert!(routed.apply("claim", &wrong).is_err(), "wrong pin resolved");

    // An uncommitted working tree has no publication pin at all.
    let head = git(&root, &["rev-parse", "HEAD"]);
    let error = message(routed.export(&head).expect_err("uncommitted export"));
    assert!(error.contains("no committed source"), "{error}");

    let checkout = Checkout::open(&root, Arc::new(SubprocessGit)).expect("checkout");
    assert!(
        checkout.git_bytes("HEAD", "code.py").is_err(),
        "a symbolic revision is not a scientific pin"
    );
}

#[test]
fn native_workflow_fixture_produces_valid_v2_records() {
    let scratch = Scratch::new("workflow");
    let root = scratch.join("owner");
    initialize(&root);
    let owner = owner(&root);
    let code = json!({"repository": "fixture", "git_revision": git(&root, &["rev-parse", "HEAD"])});
    let seed_digest = format!("sha256:{}", "d".repeat(64));
    let result_digest = format!("sha256:{}", "e".repeat(64));
    let mut appended = Vec::new();

    let program = owner
        .apply(
            "program",
            &request(
                "control-program",
                json!({"role": "program", "title": "Physics control fixture", "question": "Does the synthetic estimator pass its controls?"}),
            ),
        )
        .expect("program");
    let program_ref = pinned(&owner, &root, &program);
    appended.push(program);

    let claim = owner
        .apply(
            "claim",
            &with(
                &request(
                    "C1",
                    claim_payload("The synthetic estimator passes its declared controls."),
                ),
                &[("references", json!([program_ref]))],
            ),
        )
        .expect("claim");
    let claim_ref = pinned(&owner, &root, &claim);
    appended.push(claim.clone());

    let data = owner
        .apply(
            "artifact",
            &request("seed-plan", dataset_payload(&seed_digest)),
        )
        .expect("dataset");
    let data_ref = pinned(&owner, &root, &data);
    appended.push(data);

    let boundary = boundary_in(Duration::from_millis(600));
    let protocol = owner
        .apply(
            "preregister",
            &request(
                "P1",
                json!({"semantic": semantic(&claim_ref, &data_ref, &code, &boundary, &seed_digest)}),
            ),
        )
        .expect("protocol");
    let protocol_ref = pinned(&owner, &root, &protocol);
    appended.push(protocol);

    std::thread::sleep(Duration::from_millis(700));
    let start = owner
        .apply(
            "begin-run",
            &request(
                "E1",
                run_payload(
                    &protocol_ref,
                    &data_ref,
                    &code,
                    "running",
                    "not-run",
                    None,
                    &[],
                    &seed_digest,
                ),
            ),
        )
        .expect("run start");
    let start_ref = pinned(&owner, &root, &start);
    appended.push(start.clone());

    let output = owner
        .apply(
            "artifact",
            &request(
                "fixture-output",
                json!({
                    "role": "result",
                    "availability": "available",
                    "snapshot_digest": result_digest,
                    "locator": "fixture:result",
                    "media_type": "text/plain",
                }),
            ),
        )
        .expect("result artifact");
    let output_ref = pinned(&owner, &root, &output);
    appended.push(output);

    let completed = owner
        .apply(
            "record-run",
            &with(
                &request(
                    "E1",
                    run_payload(
                        &protocol_ref,
                        &data_ref,
                        &code,
                        "completed",
                        "passed",
                        Some(&start_ref),
                        std::slice::from_ref(&output_ref),
                        &seed_digest,
                    ),
                ),
                &[
                    ("request_id", json!("E1-finish")),
                    ("expected_heads", json!([start["revision_id"]])),
                ],
            ),
        )
        .expect("completed run");
    let completed_ref = pinned(&owner, &root, &completed);
    appended.push(completed);

    let supported = owner
        .apply(
            "assess",
            &request(
                "fixture-assessment",
                json!({
                    "claim": claim_ref,
                    "verdict": "supported",
                    "inference": "confirmatory-primary",
                    "controls": "passed",
                    "basis": "scientific-evidence",
                    "rationale": "Fixture criterion met within synthetic scope only.",
                    "evidence": [completed_ref],
                    "legacy_verdict": Value::Null,
                    "evidence_summary": "supports",
                }),
            ),
        )
        .expect("primary confirmation");
    pinned(&owner, &root, &supported);
    appended.push(supported.clone());

    let failed = owner
        .apply(
            "record-run",
            &request(
                "E-failed",
                run_payload(
                    &protocol_ref,
                    &data_ref,
                    &code,
                    "failed",
                    "failed",
                    None,
                    &[],
                    &seed_digest,
                ),
            ),
        )
        .expect("failed run");
    let failed_ref = pinned(&owner, &root, &failed);
    appended.push(failed);

    let limited = owner
        .apply(
            "assess",
            &request(
                "failed-assessment",
                json!({
                    "claim": claim_ref,
                    "verdict": "inconclusive",
                    "inference": "exploratory",
                    "controls": "failed",
                    "basis": "scientific-evidence",
                    "rationale": "Failed control; primary confirmation unavailable.",
                    "evidence": [failed_ref.clone()],
                    "legacy_verdict": Value::Null,
                    "evidence_summary": "inconclusive",
                }),
            ),
        )
        .expect("limited assessment");
    pinned(&owner, &root, &limited);
    appended.push(limited);

    // A failed run cannot be relabelled as primary confirmation by asserting a verdict.
    let refused = owner.apply(
        "assess",
        &request(
            "refused-confirmation",
            json!({
                "claim": claim_ref,
                "verdict": "supported",
                "inference": "confirmatory-primary",
                "controls": "passed",
                "basis": "scientific-evidence",
                "rationale": "Asserted confirmation over a failed run.",
                "evidence": [failed_ref],
                "legacy_verdict": Value::Null,
                "evidence_summary": "supports",
            }),
        ),
    );
    let error = match refused {
        Err(error) => message(error),
        Ok(_) => panic!("a failed run cannot support primary confirmation"),
    };
    assert!(
        error.contains("failed/pending execution or controls cannot support confirmation"),
        "{error}"
    );

    let retired = owner
        .apply(
            "retire",
            &with(
                &request("C1", claim["payload"].clone()),
                &[
                    ("request_id", json!("retire-C1")),
                    ("kind", json!("claim")),
                    ("expected_heads", json!([claim["revision_id"]])),
                ],
            ),
        )
        .expect("retirement");
    assert_eq!("retired", retired["activity"]);
    let published = pinned(&owner, &root, &retired);
    appended.push(retired.clone());

    // Every append carries the digest the contract canonicalizer computes for it.
    for record in &appended {
        assert_eq!(
            revision_digest(record).expect("digest"),
            record["revision_id"],
            "{}",
            record["id"]
        );
        assert_eq!(2, record["schema_version"]);
        assert!(validate_native(record, &[]).iter().all(|error| {
            // Reference resolution needs the closure; structure and native guards must pass.
            error.contains("resolved reference") || error.contains("confirmation")
        }));
    }

    // The retired claim keeps both verdicts and the original revision in its trace.
    let trace = owner
        .trace(
            claim["id"].as_str().expect("id"),
            claim["revision_id"].as_str().expect("revision"),
        )
        .expect("trace");
    let traced: Vec<&str> = trace["records"]
        .as_array()
        .expect("records")
        .iter()
        .filter_map(|record| record["id"].as_str())
        .collect();
    assert!(traced.contains(&supported["id"].as_str().expect("id")));
    assert_eq!(
        vec![published["revision_id"].as_str().expect("revision")],
        trace["heads"]
            .as_array()
            .expect("heads")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    );

    let export = owner
        .export(&git(&root, &["rev-parse", "HEAD"]))
        .expect("export");
    assert_eq!(
        Vec::<String>::new(),
        validate(&export, &[]),
        "export must validate against the contract"
    );
    assert!(
        export["unresolved"]
            .as_array()
            .expect("unresolved")
            .is_empty()
    );
    assert!(
        export["manifests"]
            .as_array()
            .expect("manifests")
            .iter()
            .all(|manifest| manifest["references"]
                .as_array()
                .expect("references")
                .iter()
                .all(|reference| reference["status"] == "resolved"))
    );
}

fn boundary_in(delay: Duration) -> String {
    let moment = SystemTime::now() + delay;
    let elapsed = moment.duration_since(UNIX_EPOCH).expect("after the epoch");
    // Reuse the owner clock format by formatting the same fields it emits.
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
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    })
    .unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}
