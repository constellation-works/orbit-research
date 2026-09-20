use orbit_research_core::api::Application;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git should be installed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fixture() -> (TempDir, Application) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "api-tests@example.invalid"]);
    git(root, &["config", "user.name", "API tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    let app = Application::local(root).unwrap();
    (temp, app)
}

fn create_r(app: &Application, request_key: &str) -> Value {
    app.call(
        "research.create",
        json!({
            "request_key": request_key,
            "kind": "R",
            "title": "A study",
            "body": "Question under study",
            "tags": ["api"],
        }),
    )
    .unwrap()
}

#[test]
fn local_capture_and_work_plans_need_no_backend() {
    let (temp, app) = fixture();
    let first = create_r(&app, "capture-r");
    let second = create_r(&app, "capture-r");
    assert_eq!(first, second, "request-key retry must be idempotent");
    assert_eq!(first["id"], "R001");

    let investigation = app
        .call(
            "research.plan_investigation",
            json!({"research_id":"R001","objective":"Reproduce the baseline."}),
        )
        .unwrap();
    assert_eq!(investigation["mode"], "investigation");
    assert_eq!(
        investigation["context_files"],
        json!(["dir:research/R001-a-study"])
    );

    let contribution = app
        .call(
            "research.plan_contribution",
            json!({
                "research_id":"R001",
                "unit":"control-a",
                "objective":"Measure the control."
            }),
        )
        .unwrap();
    assert_eq!(contribution["mode"], "contribution");

    let synthesis = app
        .call(
            "research.plan_synthesis",
            json!({"research_id":"R001","units":["control-a"]}),
        )
        .unwrap();
    assert_eq!(synthesis["mode"], "synthesis");
    assert!(temp.path().join(".git/orbit-research-writer").exists());
    assert!(
        !temp.path().join(".git/orbit-research-operations").exists(),
        "local capture and planning must not create Orbit operation state"
    );
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn unconfigured_backend_operations_fail_before_journal_or_task_mutation() {
    let (temp, app) = fixture();
    let error = app
        .call("research.backend", json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Orbit backend is not configured"), "{error}");

    for (operation, input) in [
        (
            "research.link_work",
            json!({
                "request_key":"link-r1",
                "title":"A task",
                "crew":"luna",
                "plan": {
                    "research_id":"R001",
                    "corpus_revision":"missing",
                    "research_blob":"missing",
                    "mode":"investigation",
                    "context_files":["dir:research/R001-study"],
                    "instructions":"objective"
                }
            }),
        ),
        (
            "research.dispatch",
            json!({"request_key":"missing","base":"main"}),
        ),
        ("research.cancel", json!({"request_key":"missing"})),
    ] {
        let error = app.call(operation, input).unwrap_err().to_string();
        assert!(
            error.contains("Orbit backend is not configured"),
            "{operation}: {error}"
        );
    }
    assert!(!temp.path().join(".git/orbit-research-operations").exists());
}

#[test]
fn operation_inputs_cannot_replace_fixed_root_or_backend_scope() {
    let (_temp, app) = fixture();
    let cases = [
        (
            "research.create",
            json!({"request_key":"r","kind":"Q","title":"Question","root":"/tmp"}),
        ),
        (
            "research.plan_investigation",
            json!({"research_id":"R001","objective":"objective","backend":"other"}),
        ),
        (
            "research.dispatch",
            json!({"request_key":"r","base":"main","task":"T1","run":"R1"}),
        ),
        (
            "research.cancel",
            json!({"request_key":"r","task":"T1","run":"R1"}),
        ),
    ];
    for (operation, input) in cases {
        let error = app.call(operation, input).unwrap_err().to_string();
        assert!(error.contains("unknown field"), "{operation}: {error}");
    }
}
