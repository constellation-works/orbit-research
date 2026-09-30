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
fn confirmed_link_retries_preserve_the_original_task() {
    let (temp, app) = fixture();
    create_r(&app, "link-reservation");
    let preparation = app.link_intent("link-request", "R001").unwrap();
    assert!(preparation.is_new);
    assert!(preparation.link.task_id.is_none());

    let first = app.link_confirm("link-request", "task-first").unwrap();
    let retry = app.link_confirm("link-request", "task-first").unwrap();
    assert_eq!(first.task_id, retry.task_id);

    let error = app
        .link_confirm("link-request", "task-competing")
        .expect_err("a confirmed request cannot be reassigned to another task");
    assert!(matches!(error, orbit_research_core::Error::Conflict(_)));
    let message = error.to_string();
    assert!(!message.contains("task-first"));
    assert!(!message.contains("task-competing"));
    let reopened = Application::local(temp.path()).unwrap();
    let recalled = reopened.link_intent("link-request", "R001").unwrap();
    assert!(!recalled.is_new);
    assert_eq!(recalled.link.task_id.as_deref(), Some("task-first"));
    let links = reopened.call("research.work_links", json!({})).unwrap();
    assert_eq!(links.as_array().unwrap().len(), 1);
    assert_eq!(links[0]["task_id"], "task-first");
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
            "research.plan",
            json!({
                "shape": "investigation",
                "research_id":"R001",
                "objective":"Reproduce the baseline."
            }),
        )
        .unwrap();
    assert_eq!(
        investigation["context_files"],
        json!(["dir:research/R001-a-study"])
    );
    assert!(
        !investigation["acceptance_criteria"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let contribution = app
        .call(
            "research.plan",
            json!({
                "shape": "contribution",
                "research_id":"R001",
                "unit":"control-a",
                "objective":"Measure the control."
            }),
        )
        .unwrap();
    assert!(
        contribution["title"]
            .as_str()
            .unwrap()
            .contains("control-a")
    );

    let synthesis = app
        .call(
            "research.plan",
            json!({"shape": "synthesis", "research_id":"R001","units":["control-a"]}),
        )
        .unwrap();
    assert!(synthesis["title"].as_str().unwrap().contains("Synthesize"));
    assert!(temp.path().join(".git/orbit-research-writer").exists());
    assert!(
        !temp.path().join(".git/orbit-research-operations").exists(),
        "local capture and planning must not create Orbit operation state"
    );
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn operation_inputs_cannot_replace_fixed_root_or_declare_unknown_fields() {
    let (_temp, app) = fixture();
    let cases = [
        (
            "research.create",
            json!({"request_key":"r","kind":"Q","title":"Question","root":"/tmp"}),
        ),
        (
            "research.plan",
            json!({"shape":"investigation","research_id":"R001","objective":"objective","backend":"other"}),
        ),
    ];
    for (operation, input) in cases {
        let error = app.call(operation, input).unwrap_err().to_string();
        assert!(error.contains("unknown field"), "{operation}: {error}");
    }
}

#[test]
fn derived_constraints_are_enforced_before_mutation() {
    use orbit_research_core::application::Operation;
    let (temp, app) = fixture();
    for input in [
        json!({"request_key":"", "kind":"Q", "title":"Question"}),
        json!({"request_key":"bad-kind", "kind":"assessment", "title":"Question"}),
        json!({"request_key":"empty-title", "kind":"Q", "title":""}),
    ] {
        assert!(app.execute(Operation::Create, input).is_err());
    }
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "1");
    assert!(!temp.path().join(".git/orbit-research-writer").exists());
    assert!(app.execute(Operation::List, json!({})).is_ok());
}

#[test]
fn check_returns_a_compact_summary_and_list_keeps_its_snapshot_schema() {
    let (temp, app) = fixture();
    create_r(&app, "check-summary");
    let base_revision = git(temp.path(), &["rev-parse", "HEAD"]);
    let status_before = git(temp.path(), &["status", "--porcelain"]);

    let check = app.call("research.check", json!({})).unwrap();
    assert_eq!(
        check,
        json!({
            "valid": true,
            "base_revision": base_revision,
            "record_count": 1,
            "tag_count": 1,
        })
    );
    assert!(check.get("records").is_none());
    assert!(check.get("body").is_none());
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), base_revision);
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), status_before);

    let list = app.call("research.list", json!({})).unwrap();
    assert_eq!(list["revision"], base_revision);
    assert_eq!(list["records"].as_array().unwrap().len(), 1);
    assert!(
        list["records"][0]["body"]
            .as_str()
            .expect("record body")
            .contains("Question under study")
    );
    assert_eq!(list["tags"], json!(["api"]));
}

#[test]
fn check_rejects_an_invalid_corpus_without_changing_git_state() {
    let (temp, app) = fixture();
    fs::write(
        temp.path().join("questions/Q001-invalid.md"),
        "not canonical frontmatter\n",
    )
    .unwrap();
    let head_before = git(temp.path(), &["rev-parse", "HEAD"]);
    let status_before = git(temp.path(), &["status", "--porcelain"]);

    let error = app.call("research.check", json!({})).unwrap_err();
    assert!(error.to_string().contains("Missing frontmatter"));
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), status_before);
}
