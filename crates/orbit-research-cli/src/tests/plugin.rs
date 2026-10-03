//! `orbit plugin test`'s conformance workspace is always an empty, non-Git
//! temporary directory with no seeding mechanism (verified against Orbit's
//! own `orbit-core/src/application/plugin/conformance.rs`), so it can only
//! exercise this backend's environment-independent refusals. These tests
//! cover what that sandbox cannot: real success output and the "unknown
//! record id" refusal against an actual git-backed corpus, by calling the
//! same `serve_plugin_tool_call` entry point the `orbit-tool` subcommand
//! serves stdin/stdout through.
use super::super::plugin::{
    AcceptInput, LinkInput, TaskHost, TaskRef, TaskState, ValidateInput, serve_plugin_tool_call,
    serve_plugin_tool_call_with_host,
};
use orbit_research_core::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../orbit-research-core/tests/fixtures/schema.json");

fn corpus() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).expect("schema directory");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("fixture schema");
    fs::write(
        root.join(".gitignore"),
        "_data/orbit-research-operations/\n.orbit-research-tmp/\n",
    )
    .expect("ignore private operational state");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("record directory");
        // Keep empty kind directories in Git, so linked worktrees have them too.
        fs::write(root.join(dir).join(".gitkeep"), "").expect("keep directory");
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    orbit_research_core::prepare_workspace_operations(root).expect("prepare fixture operations");
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
pub(super) struct FakeTaskHost {
    tasks: Mutex<Vec<(String, String)>>,
    list_calls: Mutex<usize>,
    next_id: Mutex<u32>,
    fail_create: Mutex<bool>,
    fail_put: Mutex<bool>,
    created_descriptions: Mutex<Vec<String>>,
    /// Task id -> (status, job_run_id), as `accept` reads via `orbit.task.show`.
    task_states: Mutex<BTreeMap<String, (String, Option<String>)>>,
    /// (task id, artifact path) -> stored content, as `orbit.task.artifact.put` records.
    pub(super) artifacts: Mutex<BTreeMap<(String, String), Value>>,
}

impl FakeTaskHost {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

    /// Set the task state `accept` gates on: `status` and the delivering
    /// run's id.
    pub(super) fn seed_task_state(&self, id: &str, status: &str, job_run_id: Option<&str>) {
        self.task_states
            .lock()
            .expect("lock")
            .insert(id.into(), (status.into(), job_run_id.map(String::from)));
    }

    /// Pre-store a task artifact, as if a prior call had recorded it.
    fn seed_artifact(&self, id: &str, path: &str, content: Value) {
        self.artifacts
            .lock()
            .expect("lock")
            .insert((id.into(), path.into()), content);
    }
}

impl TaskHost for FakeTaskHost {
    fn list_by_tag(&self, _workspace: &str, tag: &str) -> Result<Vec<TaskRef>> {
        *self.list_calls.lock().expect("list call count") += 1;
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
        title: &str,
        description: &str,
        _acceptance_criteria: &[String],
        _context_files: &[String],
    ) -> Result<TaskRef> {
        if *self.fail_create.lock().expect("lock") {
            return Err(Error::Internal("simulated orbit.task.add failure".into()));
        }
        // Mirror the real Orbit callback's nonblank task text requirements.
        if title.trim().is_empty() || description.trim().is_empty() {
            return Err(Error::InvalidInput(
                "task title and description must not be empty".into(),
            ));
        }
        self.created_descriptions
            .lock()
            .expect("descriptions")
            .push(description.into());
        let mut next = self.next_id.lock().expect("lock");
        *next += 1;
        let id = format!("TEST-{next}");
        self.seed(tag, &id);
        Ok(TaskRef { id })
    }

    fn task_state(&self, id: &str) -> Result<TaskState> {
        let (status, job_run_id) = self
            .task_states
            .lock()
            .expect("lock")
            .get(id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("no such task: {id}")))?;
        Ok(TaskState { status, job_run_id })
    }

    fn get_artifact(&self, id: &str, path: &str) -> Result<Option<Value>> {
        Ok(self
            .artifacts
            .lock()
            .expect("lock")
            .get(&(id.to_owned(), path.to_owned()))
            .cloned())
    }

    fn put_artifact(&self, source_path: &Path, id: &str, path: &str) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(source_path)?.permissions().mode() & 0o777,
                0o600,
                "callback source is owner-only"
            );
        }
        let bytes = fs::read(source_path)?;
        if *self.fail_put.lock().expect("lock") {
            return Err(Error::Internal(
                "simulated artifact callback failure".into(),
            ));
        }
        let content: Value = serde_json::from_slice(&bytes).map_err(|error| {
            Error::Internal(format!("staged {path} is not valid JSON: {error}"))
        })?;
        self.artifacts
            .lock()
            .expect("lock")
            .insert((id.to_owned(), path.to_owned()), content);
        Ok(())
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
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin/schemas/link.request.json");
    if std::env::var_os("ORBIT_RESEARCH_WRITE_SCHEMAS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&generated).expect("serialize schema")
            ),
        )
        .expect("write schema");
    }
    let committed: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read schema"))
        .expect("parse schema");
    assert_eq!(
        committed, generated,
        ".orbit-plugin/schemas/link.request.json has drifted from plugin.rs's LinkInput; regenerate it"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn link_refuses_unprepared_operations_before_persisting_intent_or_calling_host() {
    let temp = reserved_corpus();
    let research = orbit_research_core::Research::open(temp.path()).expect("open corpus");
    research
        .link_intent("legacy-pending", "R001")
        .expect("existing intent");
    let prepared = temp.path().join("_data/orbit-research-operations");
    let legacy = temp.path().join(".git/orbit-research-operations");
    fs::remove_file(&legacy).expect("remove private fresh marker");
    fs::rename(prepared, &legacy).expect("restore private previous layout");
    let entries = || {
        fs::read_dir(&legacy)
            .expect("legacy journal")
            .map(|entry| {
                let entry = entry.expect("journal entry");
                (
                    entry.file_name(),
                    fs::read(entry.path()).expect("journal bytes"),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = entries();
    let host = FakeTaskHost::default();
    let reply = call_with_host(
        &envelope(
            "link",
            json!({"research_id":"R001", "request_key":"new-unprepared", "title":"Investigate R001"}),
            Some(temp.path()),
        ),
        &host,
    );
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(
        reply["error"]
            .to_string()
            .contains("workspace prepare-operations"),
        "{reply}"
    );
    assert_eq!(*host.list_calls.lock().expect("list count"), 0);
    assert!(host.tasks.lock().expect("tasks").is_empty());
    assert_eq!(
        entries(),
        before,
        "refusal preserves all prior journal bytes"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn link_uses_the_title_when_optional_description_is_missing_or_blank() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::default();
    for (key, description) in [
        ("missing", None),
        ("empty", Some("")),
        ("blank", Some(" \n\t")),
    ] {
        let mut input =
            json!({"research_id":"R001", "request_key":key, "title":"Investigate R001"});
        if let Some(description) = description {
            input["description"] = json!(description);
        }
        let created = call_with_host(&envelope("link", input.clone(), Some(temp.path())), &host);
        assert_eq!(created["ok"], true, "{created}");
        assert_eq!(created["output"]["created"], true, "{created}");
        let adopted = call_with_host(&envelope("link", input, Some(temp.path())), &host);
        assert_eq!(adopted["ok"], true, "{adopted}");
        assert_eq!(adopted["output"]["created"], false, "{adopted}");
        assert_eq!(adopted["output"]["task_id"], created["output"]["task_id"]);
    }
    assert_eq!(
        *host.created_descriptions.lock().expect("descriptions"),
        vec!["Investigate R001"; 3]
    );
}

#[test]
fn link_refuses_blank_titles_before_persisting_an_intent_or_calling_the_host() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::default();
    for title in ["", " \n\t"] {
        let reply = call_with_host(
            &envelope(
                "link",
                json!({"research_id":"R001", "request_key":"invalid-title", "title":title, "description":"A valid description"}),
                Some(temp.path()),
            ),
            &host,
        );
        assert_eq!(reply["error"]["code"], "invalid_request", "{reply}");
    }
    assert!(host.tasks.lock().expect("tasks").is_empty());
    let research = orbit_research_core::Research::open(temp.path()).expect("open corpus");
    assert!(
        research.work_links().expect("work links").is_empty(),
        "an invalid title does not poison its request key with an unresolved intent"
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn link_adopts_correlated_task_when_local_intent_is_missing() {
    let temp = reserved_corpus();
    let host = FakeTaskHost::default();
    let input = json!({
        "research_id": "R001",
        "request_key": "lost-intent",
        "title": "Investigate R001",
    });

    let first = call_with_host(&envelope("link", input.clone(), Some(temp.path())), &host);
    assert_eq!(first["ok"], true, "{first:?}");
    let task_id = first["output"]["task_id"].as_str().expect("task id");

    // Model an uncertain submission followed by loss of the local log entry.
    let log_dir = temp.path().join("_data/orbit-research-operations");
    let entries: Vec<_> = fs::read_dir(log_dir)
        .expect("request log")
        .map(|entry| entry.expect("log entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(entries.len(), 1, "one link intent was saved");
    fs::remove_file(&entries[0]).expect("simulate missing local intent");

    let retry = call_with_host(&envelope("link", input, Some(temp.path())), &host);
    assert_eq!(retry["ok"], true, "{retry:?}");
    assert_eq!(retry["output"]["created"], false);
    assert_eq!(retry["output"]["task_id"], task_id);
    assert_eq!(host.tasks.lock().expect("lock").len(), 1);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[test]
fn validate_input_schema_matches_the_committed_schema() {
    let generated =
        serde_json::to_value(schemars::schema_for!(ValidateInput)).expect("serialize schema");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.orbit-plugin/schemas/validate.request.json");
    if std::env::var_os("ORBIT_RESEARCH_WRITE_SCHEMAS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&generated).expect("serialize schema")
            ),
        )
        .expect("write schema");
    }
    let committed: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read schema"))
        .expect("parse schema");
    assert_eq!(
        committed, generated,
        ".orbit-plugin/schemas/validate.request.json has drifted from plugin.rs's ValidateInput; regenerate it"
    );
}

#[test]
fn accept_input_schema_matches_the_committed_schema() {
    let generated =
        serde_json::to_value(schemars::schema_for!(AcceptInput)).expect("serialize schema");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.orbit-plugin/schemas/accept.request.json");
    if std::env::var_os("ORBIT_RESEARCH_WRITE_SCHEMAS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&generated).expect("serialize schema")
            ),
        )
        .expect("write schema");
    }
    let committed: Value = serde_json::from_str(&fs::read_to_string(&path).expect("read schema"))
        .expect("parse schema");
    assert_eq!(
        committed, generated,
        ".orbit-plugin/schemas/accept.request.json has drifted from plugin.rs's AcceptInput; regenerate it"
    );
}

/// A primary corpus with reserved `R001` and a linked run worktree whose
/// worktree-mode writer wrote a complete R001 for `task-1`/`run-1`, with one
/// local input pinned by digest.
pub(super) struct Delivered {
    pub(super) primary: TempDir,
    _parent: TempDir,
    worktree: std::path::PathBuf,
}

const INPUT: &[u8] = b"a,b\n1,2\n";
const README: &str = "research/R001-study/README.md";

pub(super) fn delivered() -> Delivered {
    let primary = reserved_corpus();
    let parent = tempfile::tempdir().expect("worktree parent");
    let worktree = parent.path().join("run");
    let output = Command::new("git")
        .arg("-C")
        .arg(primary.path())
        .args(["worktree", "add", "-q", "--detach"])
        .arg(&worktree)
        .output()
        .expect("add worktree");
    assert!(output.status.success(), "{output:?}");
    let app = orbit_research_core::Application::local(&worktree).expect("open worktree");
    let blob = app
        .call("research.show", json!({"id": "R001"}))
        .expect("show R001")["git_blob"]
        .clone();
    let written = app
        .call(
            "research.revise",
            json!({
                "id": "R001",
                "expected_blob": blob,
                "mode": "worktree",
                "status": "done",
                "orbit": {"task": "task-1", "run": "run-1"},
                "body": "## Question\n\nWhy?\n\n## Method\n\nRan it.\n\n## Result\n\nControls failed, so the run is inconclusive.\n\n## Limitations\n\nOne run.\n\n## Next\n\nRepeat.",
                "manifest": {"inputs": [{
                    "name": "input.csv",
                    "source": "fixture",
                    "sha256": "492d5ea496056f1a6a6592241032fab764c321596317930b4fa0e1e8bc3b7470",
                    "size": INPUT.len(),
                    "fetch": "generated by the fixture",
                }]},
            }),
        )
        .expect("worktree-mode write");
    assert_eq!(written["mode"], "worktree");
    let data = worktree.join("research/R001-study/data");
    fs::write(data.join("input.csv"), INPUT).expect("input bytes");
    Delivered {
        primary,
        _parent: parent,
        worktree,
    }
}

impl Delivered {
    fn edit(&self, path: &str, from: &str, to: &str) {
        let path = self.worktree.join(path);
        let text = fs::read_to_string(&path).expect("read fixture file");
        assert!(text.contains(from), "{} lacks {from:?}", path.display());
        fs::write(path, text.replacen(from, to, 1)).expect("write fixture file");
    }

    /// Call `validate` as a job step does: the checkout path is step input,
    /// while `context.workspace_root` names a different directory.
    fn validate(&self, workspace_root: &Path, run: &str) -> Value {
        call(&json!({
            "schema_version": 1,
            "tool": "research.validate",
            "input": {"path": self.worktree.to_string_lossy()},
            "context": {
                "workspace_root": workspace_root.to_string_lossy(),
                "task_id": "task-1",
                "job_run_id": run,
            },
        }))
    }

    /// Commit the worktree's write and fast-forward-merge it into the
    /// primary checkout, as the job's `git_commit`/`git_merge` steps do:
    /// `accept` reads the published commit off the primary checkout, never
    /// the run worktree.
    pub(super) fn merge_into_primary(&self) {
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.worktree)
                .args(args)
                .output()
                .expect("run fixture Git");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "Deliver R001"]);
        let head = git(&["rev-parse", "HEAD"]);
        let output = Command::new("git")
            .arg("-C")
            .arg(self.primary.path())
            .args(["merge", "-q", "--ff-only", &head])
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "git merge: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Stable golden form: the reply envelope, minus values that vary by fixture
/// run (the HEAD commit, and the README blob, which embeds today's date).
fn golden_form(mut reply: Value) -> Value {
    if let Some(output) = reply.get_mut("output").and_then(Value::as_object_mut) {
        output.remove("revision");
        output.remove("blob");
    }
    reply
}

#[test]
fn validate_goldens_cover_each_failure_reason_and_the_valid_record() {
    let elsewhere = tempfile::tempdir().expect("unrelated workspace root");
    let mut replies = serde_json::Map::new();
    let mut case = |name: &str, setup: &dyn Fn(&Delivered), run: &str| {
        let delivered = delivered();
        setup(&delivered);
        replies.insert(
            name.into(),
            golden_form(delivered.validate(elsewhere.path(), run)),
        );
    };
    case("valid", &|_| (), "run-1");
    case(
        "missing_section",
        &|d| d.edit(README, "## Limitations\n\nOne run.\n\n", ""),
        "run-1",
    );
    case(
        "placeholder_section",
        &|d| d.edit(README, "Ran it.", "Pending."),
        "run-1",
    );
    case("wrong_run_id", &|_| (), "run-2");
    case(
        "tampered_artifact_digest",
        &|d| {
            fs::write(
                d.worktree.join("research/R001-study/data/input.csv"),
                "a,b\n1,3\n",
            )
            .expect("tamper input");
        },
        "run-1",
    );
    case(
        "dangling_lineage",
        &|d| d.edit(README, "derived_from: []", "derived_from:\n- Q009"),
        "run-1",
    );
    case(
        "id_allocated_in_worktree",
        &|d| {
            let stray = d.worktree.join("research/R002-stray");
            fs::create_dir_all(&stray).expect("stray directory");
            fs::write(
                stray.join("README.md"),
                "---\nid: R002\ntitle: Stray\nstatus: planned\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: []\n---\n\nStray.\n",
            )
            .expect("stray record");
        },
        "run-1",
    );
    let actual = serde_json::to_string_pretty(&Value::Object(replies)).expect("golden JSON") + "\n";
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots/plugin-validate.json");
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::write(&golden, &actual).expect("write golden");
    }
    assert_eq!(
        actual,
        fs::read_to_string(&golden).expect("read golden"),
        "validate replies drifted from src/snapshots/plugin-validate.json; rerun with UPDATE_GOLDENS=1 and review the diff"
    );
}

#[test]
fn validate_reads_the_input_path_not_the_bound_workspace() {
    let delivered = delivered();
    // The bound workspace is the primary checkout, which still holds only the
    // unwritten stub; a valid reply proves the worktree path was checked.
    let reply = delivered.validate(delivered.primary.path(), "run-1");
    assert_eq!(reply["ok"], true, "{reply:?}");
    assert_eq!(reply["output"]["research_id"], "R001");
    // And the reverse: an invalid worktree fails even when the bound
    // workspace holds a valid corpus.
    delivered.edit(README, "Ran it.", "Pending.");
    let reply = delivered.validate(delivered.primary.path(), "run-1");
    assert_eq!(reply["error"]["code"], "section_placeholder", "{reply:?}");
    // A bound workspace that is no corpus at all does not matter either.
    let empty = tempfile::tempdir().expect("empty workspace root");
    let reply = delivered.validate(empty.path(), "run-1");
    assert_eq!(reply["error"]["code"], "section_placeholder", "{reply:?}");
}

#[test]
fn validate_refuses_without_a_run_context_or_an_absolute_path() {
    let delivered = delivered();
    let reply = call(&envelope(
        "validate",
        json!({"path": delivered.worktree.to_string_lossy()}),
        Some(delivered.primary.path()),
    ));
    assert_eq!(reply["error"]["code"], "run_context_required", "{reply:?}");
    let reply = call(&json!({
        "tool": "validate",
        "input": {"path": "relative/run"},
        "context": {"task_id": "task-1", "job_run_id": "run-1"},
    }));
    assert_eq!(reply["error"]["code"], "invalid_request", "{reply:?}");
}

/// Call `accept` bound to `primary` as the workspace, as the host resolves it
/// for any workspace-scoped mutating tool.
pub(super) fn call_accept(
    primary: &Path,
    task_id: &str,
    research_id: &str,
    host: &dyn TaskHost,
) -> Value {
    call_with_host(
        &envelope(
            "accept",
            json!({"task_id": task_id, "research_id": research_id}),
            Some(primary),
        ),
        host,
    )
}

#[test]
fn accept_preserves_a_preexisting_scratch_file() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = FakeTaskHost::default();
    host.seed_task_state("task-1", "review", Some("run-1"));
    let scratch = delivered.primary.path().join(".orbit-research-tmp");
    fs::create_dir(&scratch).expect("scratch directory");
    let existing = scratch.join("research-acceptance.json");
    fs::write(&existing, "another call's artifact").expect("existing file");

    let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(
        fs::read_to_string(existing).expect("preserved file"),
        "another call's artifact"
    );
    assert_eq!(
        fs::read_dir(scratch).expect("scratch directory").count(),
        1,
        "this call cleans up only its own staged file"
    );
}

#[test]
fn accept_cleans_up_its_private_scratch_file_when_the_callback_fails() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = FakeTaskHost::default();
    host.seed_task_state("task-1", "review", Some("run-1"));
    *host.fail_put.lock().expect("callback failure flag") = true;
    let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(reply["error"]["code"], "internal", "{reply}");
    let scratch = delivered.primary.path().join(".orbit-research-tmp");
    assert_eq!(
        fs::read_dir(&scratch).expect("scratch directory").count(),
        0
    );
    assert!(host.artifacts.lock().expect("artifacts").is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(scratch)
                .expect("scratch metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[cfg(unix)]
#[test]
fn accept_refuses_a_symlinked_scratch_directory_without_touching_its_target() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let outside = tempfile::tempdir().expect("outside scratch target");
    let existing = outside.path().join("research-acceptance.json");
    fs::write(&existing, "unrelated outside file").expect("outside file");
    std::os::unix::fs::symlink(
        outside.path(),
        delivered.primary.path().join(".orbit-research-tmp"),
    )
    .expect("scratch symlink");
    let host = FakeTaskHost::default();
    host.seed_task_state("task-1", "review", Some("run-1"));

    let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "refused", "{reply}");
    assert_eq!(
        fs::read_to_string(existing).expect("outside file survives"),
        "unrelated outside file"
    );
    assert!(host.artifacts.lock().expect("artifacts").is_empty());
}

/// A nested call completes while the first callback is still waiting to read
/// its source file, modeling two overlapping exec calls without timing waits.
struct InterleavedAcceptHost {
    host: FakeTaskHost,
    primary: std::path::PathBuf,
    entered: std::sync::atomic::AtomicBool,
}

impl TaskHost for InterleavedAcceptHost {
    fn list_by_tag(&self, workspace: &str, tag: &str) -> Result<Vec<TaskRef>> {
        self.host.list_by_tag(workspace, tag)
    }

    fn create(
        &self,
        workspace: &str,
        tag: &str,
        title: &str,
        description: &str,
        criteria: &[String],
        files: &[String],
    ) -> Result<TaskRef> {
        self.host
            .create(workspace, tag, title, description, criteria, files)
    }

    fn task_state(&self, id: &str) -> Result<TaskState> {
        self.host.task_state(id)
    }

    fn get_artifact(&self, id: &str, path: &str) -> Result<Option<Value>> {
        self.host.get_artifact(id, path)
    }

    fn put_artifact(&self, source_path: &Path, id: &str, path: &str) -> Result<()> {
        if !self.entered.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let second = call_accept(&self.primary, id, "R001", self);
            assert_eq!(second["ok"], true, "overlapping call: {second}");
        }
        self.host.put_artifact(source_path, id, path)
    }
}

#[test]
fn overlapping_accept_calls_keep_independent_callback_source_files() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = InterleavedAcceptHost {
        host: FakeTaskHost::default(),
        primary: delivered.primary.path().into(),
        entered: std::sync::atomic::AtomicBool::new(false),
    };
    host.host.seed_task_state("task-1", "review", Some("run-1"));
    let first = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(
        first["ok"], true,
        "first call still has its callback source: {first}"
    );
    assert_eq!(host.host.artifacts.lock().expect("artifacts").len(), 1);
    assert_eq!(
        fs::read_dir(delivered.primary.path().join(".orbit-research-tmp"))
            .expect("scratch")
            .count(),
        0
    );
}

/// Stable golden form: strip the published commit and blob, which vary by
/// fixture run (the commit hash and the README blob, which embeds today's
/// date).
fn accept_golden_form(mut reply: Value) -> Value {
    if let Some(output) = reply.get_mut("output").and_then(Value::as_object_mut) {
        output.remove("commit");
        output.remove("blob");
    }
    reply
}

#[test]
fn accept_goldens_cover_success_idempotent_retry_and_each_refusal() {
    let mut replies = serde_json::Map::new();

    // Success: delivery landed on the base branch and the task is `review`.
    {
        let delivered = delivered();
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "review", Some("run-1"));
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        assert_eq!(reply["output"]["recorded"], true, "{reply:?}");
        replies.insert("success".into(), accept_golden_form(reply));
    }

    // Idempotent retry: the same call again finds the stored evidence and
    // writes no second artifact.
    {
        let delivered = delivered();
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "review", Some("run-1"));
        call_accept(delivered.primary.path(), "task-1", "R001", &host);
        let retry = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        assert_eq!(retry["output"]["recorded"], false, "{retry:?}");
        assert_eq!(
            host.artifacts.lock().expect("lock").len(),
            1,
            "an identical retry must not write a second artifact"
        );
        replies.insert("idempotent_retry".into(), accept_golden_form(retry));
    }

    // Refused: the run is non-terminal (still in progress).
    {
        let delivered = delivered();
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "in-progress", Some("run-1"));
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        replies.insert("run_not_terminal".into(), accept_golden_form(reply));
    }

    // Refused: the run failed (the task never reached review; it's blocked).
    {
        let delivered = delivered();
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "blocked", Some("run-1"));
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        replies.insert("run_failed".into(), accept_golden_form(reply));
    }

    // Refused: before delivery lands, the primary checkout still holds only
    // the reserved stub, which fails validate.
    {
        let delivered = delivered();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "review", Some("run-1"));
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        replies.insert("before_delivery_lands".into(), accept_golden_form(reply));
    }

    // Refused: delivery landed, but the published commit itself fails
    // validate (a placeholder section slipped past the run's own gate).
    {
        let delivered = delivered();
        delivered.edit(README, "Ran it.", "Pending.");
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "review", Some("run-1"));
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        replies.insert(
            "validate_fails_at_published_commit".into(),
            accept_golden_form(reply),
        );
    }

    // Refused: a different acceptance is already stored for this task.
    {
        let delivered = delivered();
        delivered.merge_into_primary();
        let host = FakeTaskHost::default();
        host.seed_task_state("task-1", "review", Some("run-1"));
        host.seed_artifact(
            "task-1",
            "research-acceptance.json",
            json!({
                "research_id": "R001",
                "commit": "0".repeat(40),
                "blob": "0".repeat(40),
                "run_id": "run-0",
                "artifact_digests": {},
            }),
        );
        let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
        replies.insert("stored_evidence_mismatch".into(), accept_golden_form(reply));
    }

    let actual = serde_json::to_string_pretty(&Value::Object(replies)).expect("golden JSON") + "\n";
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots/plugin-accept.json");
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::write(&golden, &actual).expect("write golden");
    }
    assert_eq!(
        actual,
        fs::read_to_string(&golden).expect("read golden"),
        "accept replies drifted from src/snapshots/plugin-accept.json; rerun with UPDATE_GOLDENS=1 and review the diff"
    );
}

/// Commit an unrelated file on the primary checkout, as any later corpus
/// write (another record, a revision, a maintenance commit) would.
fn commit_unrelated_change(primary: &Path) {
    fs::write(primary.join("NOTES.txt"), "unrelated\n").expect("unrelated file");
    for args in [
        &["add", "-A"][..],
        &["commit", "-q", "-m", "Unrelated corpus commit"][..],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(primary)
            .args(args)
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn stored_acceptance(host: &FakeTaskHost) -> Value {
    host.artifacts
        .lock()
        .expect("artifacts")
        .get(&("task-1".to_owned(), "research-acceptance.json".to_owned()))
        .cloned()
        .expect("stored acceptance")
}

#[test]
fn accept_retry_after_an_unrelated_commit_is_idempotent_and_keeps_the_first_commit() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = FakeTaskHost::default();
    host.seed_task_state("task-1", "review", Some("run-1"));
    let first = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(first["output"]["recorded"], true, "{first}");
    let stored = stored_acceptance(&host);

    commit_unrelated_change(delivered.primary.path());
    let retry = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(retry["ok"], true, "{retry}");
    assert_eq!(retry["output"]["recorded"], false, "{retry}");
    assert_eq!(
        retry["output"]["commit"], first["output"]["commit"],
        "the stored commit is returned, not the new HEAD"
    );
    assert_eq!(retry["output"]["blob"], first["output"]["blob"]);
    assert_eq!(
        stored_acceptance(&host),
        stored,
        "a retry never rewrites the stored artifact"
    );
    assert_eq!(host.artifacts.lock().expect("artifacts").len(), 1);
}

/// Accept, then rewrite one stored evidence field, so the retry's freshly
/// derived evidence differs from what the task carries in exactly that field.
fn accept_conflict_after_tampering(field: &str, value: Value) -> Value {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = FakeTaskHost::default();
    host.seed_task_state("task-1", "review", Some("run-1"));
    let first = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(first["output"]["recorded"], true, "{first}");
    let mut stored = stored_acceptance(&host);
    stored[field] = value;
    host.seed_artifact("task-1", "research-acceptance.json", stored.clone());
    let reply = call_accept(delivered.primary.path(), "task-1", "R001", &host);
    assert_eq!(
        stored_acceptance(&host),
        stored,
        "a refused retry leaves the stored artifact alone"
    );
    reply
}

#[test]
fn accept_refuses_a_stored_blob_mismatch_and_names_the_blob() {
    let reply = accept_conflict_after_tampering("blob", json!("0".repeat(40)));
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "conflict", "{reply}");
    let message = reply["error"]["message"].as_str().expect("message");
    assert!(message.contains("(blob)"), "{message}");
}

#[test]
fn accept_refuses_a_stored_run_mismatch_and_names_the_run() {
    let reply = accept_conflict_after_tampering("run_id", json!("run-0"));
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "conflict", "{reply}");
    let message = reply["error"]["message"].as_str().expect("message");
    assert!(message.contains("(run_id)"), "{message}");
}

#[test]
fn accept_refuses_a_stored_digest_mismatch_and_names_the_digests() {
    let reply = accept_conflict_after_tampering("artifact_digests", json!({"input.csv": "00"}));
    assert_eq!(reply["error"]["code"], "conflict", "{reply}");
    let message = reply["error"]["message"].as_str().expect("message");
    assert!(message.contains("(artifact_digests)"), "{message}");
}

#[test]
fn accept_refuses_an_unknown_task() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = FakeTaskHost::default();
    let reply = call_accept(delivered.primary.path(), "task-unknown", "R001", &host);
    assert_eq!(reply["ok"], false, "{reply:?}");
}
