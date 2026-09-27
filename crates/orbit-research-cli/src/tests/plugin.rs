//! `orbit plugin test`'s conformance workspace is always an empty, non-Git
//! temporary directory with no seeding mechanism (verified against Orbit's
//! own `orbit-core/src/application/plugin/conformance.rs`), so it can only
//! exercise this backend's environment-independent refusals. These tests
//! cover what that sandbox cannot: real success output and the "unknown
//! record id" refusal against an actual git-backed corpus, by calling the
//! same `serve_plugin_tool_call` entry point the `orbit-tool` subcommand
//! serves stdin/stdout through.
use super::super::plugin::{
    LinkInput, TaskHost, TaskRef, serve_plugin_tool_call, serve_plugin_tool_call_with_host,
};
use orbit_research_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../orbit-research-core/tests/fixtures/schema.json");

fn corpus() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).expect("schema directory");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("fixture schema");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("record directory");
    }
    fs::write(
        root.join("questions/Q001-why.md"),
        "---\nid: Q001\ntitle: Why\nstatus: answered\ntags: [logic, evidence]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-02\nanswered_by: []\n---\nQuestion body.",
    )
    .expect("fixture record");
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "plugin-tests@example.invalid"]);
    run(&["config", "user.name", "Plugin tests"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "fixture"]);
    temp
}

fn call(request: &Value) -> Value {
    let mut output = Vec::new();
    serve_plugin_tool_call(Cursor::new(request.to_string().into_bytes()), &mut output)
        .expect("serve one plugin tool call");
    serde_json::from_slice(&output).expect("one JSON reply")
}

fn call_with_host(request: &Value, host: &dyn TaskHost) -> Value {
    let mut output = Vec::new();
    serve_plugin_tool_call_with_host(
        Cursor::new(request.to_string().into_bytes()),
        &mut output,
        host,
    )
    .expect("serve one plugin tool call");
    serde_json::from_slice(&output).expect("one JSON reply")
}

/// A fixture corpus with a reserved `R001`, for `plan`/`link` tests that need
/// an existing research item.
fn reserved_corpus() -> TempDir {
    let temp = corpus();
    let research = orbit_research_core::Research::open(temp.path()).expect("open corpus");
    research
        .reserve("r1", "R", "Study", "Question under study", vec![], vec![])
        .expect("reserve R001");
    temp
}

/// An in-memory stand-in for Orbit's `orbit.task.list`/`orbit.task.add`
/// callbacks, so `link` goldens can exercise success, retry and refusal
/// without a real Orbit installation (see DANI-10707's orchestrator note).
#[derive(Default)]
struct FakeTaskHost {
    tasks: Mutex<Vec<(String, String)>>,
    next_id: Mutex<u32>,
    fail_create: Mutex<bool>,
}

impl FakeTaskHost {
    fn failing() -> Self {
        let host = Self::default();
        *host.fail_create.lock().expect("lock") = true;
        host
    }

    fn seed(&self, tag: &str, id: &str) {
        self.tasks
            .lock()
            .expect("lock")
            .push((tag.into(), id.into()));
    }
}

impl TaskHost for FakeTaskHost {
    fn list_by_tag(&self, _workspace: &str, tag: &str) -> Result<Vec<TaskRef>> {
        Ok(self
            .tasks
            .lock()
            .expect("lock")
            .iter()
            .filter(|(existing_tag, _)| existing_tag == tag)
            .map(|(_, id)| TaskRef { id: id.clone() })
            .collect())
    }

    fn create(
        &self,
        _workspace: &str,
        tag: &str,
        _title: &str,
        _description: &str,
        _acceptance_criteria: &[String],
        _context_files: &[String],
    ) -> Result<TaskRef> {
        if *self.fail_create.lock().expect("lock") {
            return Err(Error::Internal("simulated orbit.task.add failure".into()));
        }
        let mut next = self.next_id.lock().expect("lock");
        *next += 1;
        let id = format!("TEST-{next}");
        self.seed(tag, &id);
        Ok(TaskRef { id })
    }
}

fn envelope(tool: &str, input: Value, workspace_root: Option<&Path>) -> Value {
    let mut request = json!({"schema_version": 1, "tool": tool, "input": input});
    if let Some(root) = workspace_root {
        request["context"] = json!({"workspace_root": root.to_string_lossy()});
    }
    request
}

#[test]
fn version_succeeds_with_no_bound_workspace() {
    let reply = call(&envelope("version", json!({}), None));
    assert_eq!(reply["ok"], true);
    assert_eq!(
        reply["output"]["core_version"],
        orbit_research_core::VERSION
    );
}

#[test]
fn list_reads_the_fixture_corpus() {
    let temp = corpus();
    let reply = call(&envelope("research.list", json!({}), Some(temp.path())));
    assert_eq!(reply["ok"], true);
    let records = reply["output"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "Q001");
}

#[test]
fn check_reports_the_fixture_summary() {
    let temp = corpus();
    let reply = call(&envelope(
        "orbit.research.check",
        json!({}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["output"]["valid"], true);
    assert_eq!(reply["output"]["record_count"], 1);
}

#[test]
fn show_returns_the_matching_record() {
    let temp = corpus();
    let reply = call(&envelope("show", json!({"id": "Q001"}), Some(temp.path())));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["output"]["id"], "Q001");
    assert_eq!(reply["output"]["body"], "Question body.");
}

#[test]
fn show_refuses_an_unknown_id_with_a_typed_error() {
    let temp = corpus();
    let reply = call(&envelope("show", json!({"id": "Q999"}), Some(temp.path())));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "record_not_found");
    assert_eq!(reply["error"]["retryable"], false);
    assert!(
        reply["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("Q999")
    );
}

#[test]
fn show_refuses_a_malformed_id_as_an_invalid_request() {
    let temp = corpus();
    let reply = call(&envelope(
        "show",
        json!({"id": "nonsense"}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "invalid_request");
}

#[test]
fn unknown_tool_is_refused_as_an_invalid_request() {
    let temp = corpus();
    let reply = call(&envelope(
        "research.frobnicate",
        json!({}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "invalid_request");
}

#[test]
fn a_workspace_scoped_tool_without_a_bound_workspace_is_refused() {
    let reply = call(&envelope("list", json!({}), None));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "workspace_required");
}

#[test]
fn an_unopenable_workspace_is_a_typed_corpus_refusal_not_a_crash() {
    let empty = tempfile::tempdir().expect("empty directory");
    let reply = call(&envelope("check", json!({}), Some(empty.path())));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "corpus_unavailable");
}

#[test]
fn plan_drafts_an_investigation_task_for_the_reserved_item() {
    let temp = reserved_corpus();
    let reply = call(&envelope(
        "plan",
        json!({
            "shape": "investigation",
            "research_id": "R001",
            "objective": "Reproduce the baseline."
        }),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], true);
    assert_eq!(
        reply["output"]["context_files"],
        json!(["dir:research/R001-study"])
    );
    assert!(
        reply["output"]["title"]
            .as_str()
            .expect("title")
            .contains("R001")
    );
    assert!(
        !reply["output"]["acceptance_criteria"]
            .as_array()
            .expect("criteria")
            .is_empty()
    );
}

#[test]
fn link_input_schema_matches_the_committed_schema() {
    let generated =
        serde_json::to_value(schemars::schema_for!(LinkInput)).expect("serialize schema");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/link.request.json");
    let committed: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read schema"))
        .expect("parse schema");
    assert_eq!(
        committed, generated,
        "schemas/link.request.json has drifted from plugin.rs's LinkInput; regenerate it"
    );
}

#[test]
fn link_creates_exactly_one_task_and_an_identical_retry_adopts_it() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::default();
    let input = json!({
        "research_id": "R001",
        "request_key": "link-1",
        "title": "Investigate R001",
        "description": "Reproduce the baseline.",
        "acceptance_criteria": ["README documents the result"],
    });

    let first = call_with_host(&envelope("link", input.clone(), Some(temp.path())), &host);
    assert_eq!(first["ok"], true, "{first:?}");
    assert_eq!(first["output"]["created"], true);
    let task_id = first["output"]["task_id"]
        .as_str()
        .expect("task_id")
        .to_owned();

    let retry = call_with_host(&envelope("link", input, Some(temp.path())), &host);
    assert_eq!(retry["ok"], true, "{retry:?}");
    assert_eq!(retry["output"]["created"], false);
    assert_eq!(retry["output"]["task_id"], json!(task_id));
    assert_eq!(
        host.tasks.lock().expect("lock").len(),
        1,
        "an identical retry must create no additional task"
    );
}

#[test]
fn link_refuses_when_the_research_item_is_not_reserved() {
    let temp = corpus();
    let host = FakeTaskHost::default();
    let reply = call_with_host(
        &envelope(
            "link",
            json!({"research_id": "R001", "request_key": "k", "title": "t"}),
            Some(temp.path()),
        ),
        &host,
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "record_not_found");
}

#[test]
fn link_refuses_when_more_than_one_task_carries_the_key() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::default();
    host.seed("research-request:dup", "TASK-A");
    host.seed("research-request:dup", "TASK-B");
    let reply = call_with_host(
        &envelope(
            "link",
            json!({"research_id": "R001", "request_key": "dup", "title": "t"}),
            Some(temp.path()),
        ),
        &host,
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "conflict");
}

#[test]
fn link_refuses_when_a_recorded_intent_has_no_matching_task() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::failing();
    let input = json!({"research_id": "R001", "request_key": "broken", "title": "t"});

    let first = call_with_host(&envelope("link", input.clone(), Some(temp.path())), &host);
    assert_eq!(
        first["ok"], false,
        "the simulated create failure must surface"
    );

    let retry = call_with_host(&envelope("link", input, Some(temp.path())), &host);
    assert_eq!(retry["ok"], false);
    assert_eq!(retry["error"]["code"], "conflict");
    assert!(
        retry["error"]["message"]
            .as_str()
            .expect("message")
            .contains("recorded")
    );
}
