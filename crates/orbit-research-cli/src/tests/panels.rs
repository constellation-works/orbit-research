//! Panel sources over real Git-backed fixture corpora, through the same
//! `serve_plugin_tool_call_with_host` entry point the `orbit-tool` subcommand
//! serves. Orbit's own conformance run can only reach the no-corpus state, so
//! rows, empty states and the task callbacks are pinned here.
use super::super::panels::{VERBS, serve};
use super::super::plugin::{TaskHost, TaskRef, TaskState, serve_plugin_tool_call_with_host};
use orbit_research_core::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../orbit-research-core/tests/fixtures/schema.json");

/// A Git-backed corpus holding exactly `records` (corpus-relative path, text).
fn corpus_with(records: &[(String, String)]) -> TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).expect("schema directory");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("fixture schema");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("record directory");
        fs::write(root.join(dir).join(".gitkeep"), "").expect("keep directory");
    }
    for (path, text) in records {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("record directory")).expect("record directory");
        fs::write(path, text).expect("fixture record");
    }
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "panel-tests@example.invalid"]);
    git(&["config", "user.name", "Panel tests"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "fixture"]);
    temp
}

fn question(
    id: &str,
    slug: &str,
    title: &str,
    status: &str,
    tags: &str,
    answered_by: &str,
) -> (String, String) {
    (
        format!("questions/{id}-{slug}.md"),
        format!(
            "---\nid: {id}\nslug: {slug}\ntitle: {title}\nstatus: {status}\ntags: {tags}\nderived_from: []\ncreated: 2026-09-01\nupdated: 2026-09-12\nanswered_by: {answered_by}\n---\nBody.\n"
        ),
    )
}

fn result(
    id: &str,
    slug: &str,
    status: &str,
    tests: &str,
    derived: &str,
    orbit: Option<(&str, &str)>,
) -> (String, String) {
    let orbit = orbit.map_or(String::new(), |(task, run)| {
        format!("orbit:\n  task: {task}\n  run: {run}\n")
    });
    (
        format!("research/{id}-{slug}/README.md"),
        format!(
            "---\nid: {id}\nslug: {slug}\ntitle: Result {slug}\nstatus: {status}\ntags: []\nderived_from: {derived}\ncreated: 2026-09-02\nupdated: 2026-09-15\ntests: {tests}\n{orbit}---\nBody.\n"
        ),
    )
}

const ASSESSMENTS: [&str; 4] = [
    "  - {date: 2026-09-05, research: R001, revision: 1, verdict: inconclusive, strength: anecdote}",
    "  - {date: 2026-09-06, research: R001, revision: 1, verdict: supports, strength: suggestive}",
    "  - {date: 2026-09-14, research: R001, revision: 2, verdict: supports, strength: suggestive}",
    "  - {date: 2026-09-15, research: R002, revision: 2, verdict: refutes, strength: strong, note: Control failed in the rerun.}",
];

/// Three questions (two open), two hypotheses (one with two results that
/// disagree on its current revision), three results (two delivered).
fn research_corpus() -> TempDir {
    let long = "Why does the macOS runner flake on the cache warm-up step only when the workspace was restored from an older snapshot";
    let records = [
        question("Q001", "flaky-ci", "Why does CI flake on macOS", "open", "[ci, evidence]", "[]"),
        question("Q002", "retries", "Do retries help", "answered", "[logic]", "[R001]"),
        question("Q003", "long", long, "open", "[]", "[]"),
        (
            "hypotheses/H001-retries-hide-it.md".to_owned(),
            format!(
                "---\nid: H001\nslug: retries-hide-it\ntitle: Retries hide the flake\nstatus: inconclusive\ntags: [ci]\nderived_from: [Q001]\ncreated: 2026-09-03\nupdated: 2026-09-15\nrevision: 2\nassessments:\n{}\n---\nBody.\n",
                ASSESSMENTS.join("\n")
            ),
        ),
        (
            "hypotheses/H002-warm-up.md".to_owned(),
            "---\nid: H002\nslug: warm-up\ntitle: Cache warm-up dominates\nstatus: open\ntags: []\nderived_from: [Q003]\ncreated: 2026-09-03\nupdated: 2026-09-03\nrevision: 1\nassessments: []\n---\nBody.\n".to_owned(),
        ),
        result("R001", "baseline", "done", "[H001]", "[Q002]", Some(("ORB-1", "jrun-1"))),
        result("R002", "rerun", "done", "[H001]", "[Q001]", Some(("ORB-2", "jrun-2"))),
        result("R003", "warm", "planned", "[H002]", "[Q003]", None),
    ];
    corpus_with(&records)
}

/// An in-memory `orbit.task.show` + `orbit.task.artifact.get` stand-in. Only
/// artifact reads are meaningful to the panels; everything else is unused.
#[derive(Default)]
struct ArtifactHost {
    artifacts: BTreeMap<String, Value>,
    failing: bool,
    reads: Mutex<Vec<String>>,
}

impl ArtifactHost {
    fn accepted(task: &str, research: &str) -> Self {
        let mut host = Self::default();
        host.artifacts
            .insert(task.into(), json!({"research_id": research}));
        host
    }
}

impl TaskHost for ArtifactHost {
    fn list_by_tag(&self, _: &str, _: &str) -> Result<Vec<TaskRef>> {
        Err(Error::Internal("a panel never lists tasks".into()))
    }
    fn create(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &[String],
        _: &[String],
    ) -> Result<TaskRef> {
        Err(Error::Internal("a panel never creates tasks".into()))
    }
    fn task_state(&self, _: &str) -> Result<TaskState> {
        Err(Error::Internal("a panel never reads task state".into()))
    }
    fn get_artifact(&self, id: &str, path: &str) -> Result<Option<Value>> {
        assert_eq!(path, "research-acceptance.json");
        self.reads.lock().expect("reads").push(id.into());
        if self.failing {
            return Err(Error::Internal("simulated callback refusal".into()));
        }
        Ok(self.artifacts.get(id).cloned())
    }
    fn put_artifact(&self, _: &Path, _: &str, _: &str) -> Result<()> {
        Err(Error::Internal("a panel never writes artifacts".into()))
    }
}

fn panel(verb: &str, root: &Path, host: &dyn TaskHost) -> Value {
    let reply = serve(verb, json!({}), root, host);
    assert_eq!(reply["ok"], true, "{verb}: {reply}");
    reply["output"].clone()
}

fn rows(output: &Value) -> &Vec<Value> {
    output
        .as_array()
        .expect("a table output is an array of rows")
}

#[test]
fn open_questions_lists_open_questions_with_tags_and_linked_tasks() {
    let temp = research_corpus();
    let output = panel("open-questions", temp.path(), &ArtifactHost::default());
    assert_eq!(
        output,
        json!([
            {"id": "Q001", "question": "Why does CI flake on macOS", "tags": "ci, evidence",
             "tasks": "ORB-1, ORB-2", "updated": "2026-09-12"},
            {"id": "Q003",
             "question": "Why does the macOS runner flake on the cache warm-up step only when the workspa…",
             "tags": null, "tasks": null, "updated": "2026-09-12"},
        ])
    );
}

#[test]
fn hypotheses_show_every_revision_and_never_collapse_disagreeing_results() {
    let temp = research_corpus();
    let output = panel("hypotheses", temp.path(), &ArtifactHost::default());
    assert_eq!(
        output,
        json!([
            {"id": "H001", "name": "Retries hide the flake", "rev": "2 (current, disputed)",
             "status": "inconclusive", "verdict": "supports (suggestive)", "via": "R001", "when": "2026-09-14"},
            {"id": "H001", "name": "Retries hide the flake", "rev": "2 (current, disputed)",
             "status": "inconclusive", "verdict": "refutes (strong)", "via": "R002", "when": "2026-09-15"},
            // R001 first said inconclusive, then supports, on revision 1:
            // only its latest verdict on that revision is shown.
            {"id": "H001", "name": "Retries hide the flake", "rev": "1 (superseded)",
             "status": "inconclusive", "verdict": "supports (suggestive)", "via": "R001", "when": "2026-09-06"},
            {"id": "H002", "name": "Cache warm-up dominates", "rev": "1 (current)",
             "status": "open", "verdict": "not assessed", "via": null, "when": null},
        ])
    );
}

#[test]
fn agreeing_results_on_one_revision_are_not_marked_disputed() {
    let records = [
        question("Q001", "q", "Q", "open", "[]", "[]"),
        (
            "hypotheses/H001-h.md".to_owned(),
            "---\nid: H001\nslug: h\ntitle: H\nstatus: supported\ntags: []\nderived_from: [Q001]\ncreated: 2026-09-03\nupdated: 2026-09-15\nrevision: 1\nassessments:\n  - {date: 2026-09-05, research: R001, revision: 1, verdict: supports, strength: strong}\n  - {date: 2026-09-06, research: R002, revision: 1, verdict: supports, strength: anecdote}\n---\nBody.\n".to_owned(),
        ),
        result("R001", "a", "done", "[H001]", "[Q001]", None),
        result("R002", "b", "done", "[H001]", "[Q001]", None),
    ];
    let temp = corpus_with(&records);
    let output = panel("hypotheses", temp.path(), &ArtifactHost::default());
    let revisions: Vec<_> = rows(&output).iter().map(|row| row["rev"].clone()).collect();
    assert_eq!(revisions, [json!("1 (current)"), json!("1 (current)")]);
}

#[test]
fn corpus_health_reports_validity_revision_and_counts_by_status() {
    let temp = research_corpus();
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("HEAD")
            .stdout,
    )
    .expect("utf-8");
    assert_eq!(
        panel("corpus-health", temp.path(), &ArtifactHost::default()),
        json!({
            "Corpus": "Valid",
            "Hypotheses": "2 (1 open, 1 inconclusive)",
            "Questions": "3 (2 open, 1 answered)",
            "Records": 8,
            "Results": "3 (1 planned, 2 done)",
            "Revision": &head.trim()[..12],
            "Tags": 3,
            "Theories": "0",
        })
    );
}

#[test]
fn awaiting_acceptance_omits_accepted_results_and_lists_the_rest() {
    let temp = research_corpus();
    let host = ArtifactHost::accepted("ORB-1", "R001");
    assert_eq!(
        panel("awaiting-acceptance", temp.path(), &host),
        json!([{"id": "R002", "result": "Result rerun", "status": "awaiting acceptance",
                "task": "ORB-2", "updated": "2026-09-15"}])
    );
    // R003 is only reserved: no task or run, so it is never read.
    assert_eq!(*host.reads.lock().expect("reads"), ["ORB-1", "ORB-2"]);
}

#[test]
fn awaiting_acceptance_never_calls_an_unread_result_accepted() {
    let temp = research_corpus();
    let host = ArtifactHost {
        failing: true,
        ..ArtifactHost::default()
    };
    assert_eq!(
        panel("awaiting-acceptance", temp.path(), &host),
        json!([
            {"id": "R001", "result": "Result baseline", "status": "acceptance unknown",
             "task": "ORB-1", "updated": "2026-09-15"},
            {"id": "R002", "result": "Result rerun", "status": "acceptance unknown",
             "task": "ORB-2", "updated": "2026-09-15"},
        ])
    );
}

#[test]
fn awaiting_acceptance_flags_an_artifact_that_names_another_result() {
    let temp = research_corpus();
    let mut host = ArtifactHost::accepted("ORB-1", "R001");
    host.artifacts
        .insert("ORB-2".into(), json!({"research_id": "R001"}));
    let output = panel("awaiting-acceptance", temp.path(), &host);
    assert_eq!(rows(&output).len(), 1, "{output}");
    assert_eq!(output[0]["status"], "acceptance names another result");
}

#[test]
fn awaiting_acceptance_stops_calling_back_after_repeated_failures() {
    let mut records = Vec::new();
    for number in 1..=6 {
        records.push(result(
            &format!("R{number:03}"),
            &format!("r{number}"),
            "done",
            "[]",
            "[]",
            Some((&format!("ORB-{number}"), &format!("jrun-{number}"))),
        ));
    }
    let temp = corpus_with(&records);
    let host = ArtifactHost {
        failing: true,
        ..ArtifactHost::default()
    };
    let output = panel("awaiting-acceptance", temp.path(), &host);
    assert_eq!(rows(&output).len(), 6);
    assert!(
        rows(&output)
            .iter()
            .all(|row| row["status"] == "acceptance unknown")
    );
    assert_eq!(host.reads.lock().expect("reads").len(), 3);
}

#[test]
fn awaiting_acceptance_only_counts_results_committed_at_head() {
    let temp = research_corpus();
    let path = temp.path().join("research/R003-warm/README.md");
    let text = fs::read_to_string(&path).expect("R003");
    fs::write(
        &path,
        text.replace(
            "---\nBody",
            "orbit:\n  task: ORB-3\n  run: jrun-3\n---\nBody",
        ),
    )
    .expect("uncommitted edit");
    let host = ArtifactHost::accepted("ORB-1", "R001");
    let output = panel("awaiting-acceptance", temp.path(), &host);
    assert_eq!(output[0]["id"], "R002");
    assert_eq!(rows(&output).len(), 1, "{output}");
    assert!(
        !host
            .reads
            .lock()
            .expect("reads")
            .contains(&"ORB-3".to_owned())
    );
}

#[test]
fn an_empty_corpus_gets_one_intentional_status_row_per_table() {
    let temp = tempfile::tempdir().expect("temporary directory");
    orbit_research_core::init_workspace(temp.path()).expect("initialize empty corpus");
    let host = ArtifactHost::default();
    assert_eq!(
        panel("open-questions", temp.path(), &host),
        json!([{"status": "No questions captured yet. Capture one with `orbit-research research capture`."}])
    );
    assert_eq!(
        panel("awaiting-acceptance", temp.path(), &host),
        json!([{"status": "No delivered results yet. A result appears here once its investigation run commits it."}])
    );
    assert_eq!(
        panel("hypotheses", temp.path(), &host),
        json!([{"status": "No hypotheses yet. Create one with `orbit-research research create --kind H`."}])
    );
    let health = panel("corpus-health", temp.path(), &host);
    assert_eq!(health["Corpus"], "Valid");
    assert_eq!(health["Records"], 0);
    assert_eq!(health["Questions"], "0");
    assert!(host.reads.lock().expect("reads").is_empty());
}

#[test]
fn a_corpus_without_open_questions_or_pending_results_says_so() {
    let records = [
        question("Q001", "done", "Done", "answered", "[]", "[R001]"),
        result(
            "R001",
            "a",
            "done",
            "[]",
            "[Q001]",
            Some(("ORB-1", "jrun-1")),
        ),
    ];
    let temp = corpus_with(&records);
    let host = ArtifactHost::accepted("ORB-1", "R001");
    assert_eq!(
        panel("open-questions", temp.path(), &host),
        json!([{"status": "No open questions. The only question is answered or dropped."}])
    );
    assert_eq!(
        panel("awaiting-acceptance", temp.path(), &host),
        json!([{"status": "Nothing awaiting acceptance. The only delivered result is accepted."}])
    );
}

#[test]
fn a_workspace_without_a_corpus_shows_a_short_message_not_an_error() {
    let temp = tempfile::tempdir().expect("empty workspace");
    let message = "This workspace has no research corpus yet. Run `orbit-research workspace init <path>` to create one.";
    for verb in ["open-questions", "awaiting-acceptance", "hypotheses"] {
        assert_eq!(
            panel(verb, temp.path(), &ArtifactHost::default()),
            json!([{"status": message}]),
            "{verb}"
        );
    }
    assert_eq!(
        panel("corpus-health", temp.path(), &ArtifactHost::default()),
        json!({"Corpus": "No corpus", "Detail": message})
    );
}

#[test]
fn an_invalid_corpus_names_the_problem_without_a_raw_error_dump() {
    // Q002 without Q001: the whole-corpus numbering rule refuses the snapshot.
    let records = [question("Q002", "gap", "Gap", "open", "[]", "[]")];
    let temp = corpus_with(&records);
    let host = ArtifactHost::default();
    let table = panel("open-questions", temp.path(), &host);
    let message = table[0]["status"].as_str().expect("status message");
    assert_eq!(
        message,
        "The research corpus cannot be read: Non-monotonic IDs: expected Q001, found Q002. Run `orbit-research research check` for details."
    );
    let health = panel("corpus-health", temp.path(), &host);
    assert_eq!(health["Corpus"], "Invalid");
    assert_eq!(health["Detail"], message);
}

#[test]
fn every_cell_is_a_flat_scalar_and_every_table_column_is_stable() {
    let temp = research_corpus();
    let host = ArtifactHost::accepted("ORB-1", "R001");
    for verb in VERBS {
        let output = panel(verb, temp.path(), &host);
        let objects: Vec<&Value> = match &output {
            Value::Array(rows) => rows.iter().collect(),
            object => vec![object],
        };
        for object in objects {
            let keys: Vec<&String> = object.as_object().expect("row object").keys().collect();
            let mut sorted = keys.clone();
            sorted.sort();
            // Orbit re-parses this reply without insertion order, so alphabetical
            // order is the order the dashboard shows.
            assert_eq!(keys, sorted, "{verb}");
            for (key, cell) in object.as_object().expect("row object") {
                assert!(
                    cell.is_string() || cell.is_number() || cell.is_null(),
                    "{verb}.{key} is not a flat scalar: {cell}"
                );
            }
        }
        if let Value::Array(rows) = &output {
            let columns: Vec<_> = rows[0].as_object().expect("row").keys().collect();
            assert!(
                rows.iter().all(|row| row
                    .as_object()
                    .expect("row")
                    .keys()
                    .eq(columns.iter().copied())),
                "{verb}: every row has the same columns"
            );
        }
    }
}

#[test]
fn dates_are_plain_calendar_dates_and_titles_are_one_line() {
    let temp = research_corpus();
    let output = panel("open-questions", temp.path(), &ArtifactHost::default());
    for row in rows(&output) {
        let updated = row["updated"].as_str().expect("date");
        assert_eq!(updated.len(), 10, "{updated}");
        let question = row["question"].as_str().expect("question");
        assert!(
            question.chars().count() <= 80 && !question.contains('\n'),
            "{question}"
        );
    }
}

#[test]
fn panel_verbs_route_through_the_plugin_envelope_with_any_namespace_spelling() {
    let temp = research_corpus();
    let host = ArtifactHost::default();
    for tool in [
        "open-questions",
        "research.open-questions",
        "orbit.research.open-questions",
    ] {
        let request = json!({
            "schema_version": 1,
            "tool": tool,
            "input": {},
            "context": {"workspace_root": temp.path().to_string_lossy()},
        });
        let mut output = Vec::new();
        serve_plugin_tool_call_with_host(
            Cursor::new(request.to_string().into_bytes()),
            &mut output,
            &host,
        )
        .expect("serve panel call");
        let reply: Value = serde_json::from_slice(&output).expect("JSON reply");
        assert_eq!(reply["ok"], true, "{tool}: {reply}");
        assert_eq!(reply["output"][0]["id"], "Q001", "{tool}");
    }
}

#[test]
fn panel_tools_refuse_unknown_input_and_never_write() {
    let temp = research_corpus();
    let head = || {
        Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["status", "--porcelain", "--untracked-files=all"])
            .output()
            .expect("status")
            .stdout
    };
    for verb in VERBS {
        let reply = serve(
            verb,
            json!({"unexpected": true}),
            temp.path(),
            &ArtifactHost::default(),
        );
        assert_eq!(reply["ok"], false, "{verb}");
        assert_eq!(reply["error"]["code"], "invalid_request", "{verb}");
        panel(verb, temp.path(), &ArtifactHost::default());
    }
    assert!(head().is_empty(), "panel reads leave the corpus untouched");
}

#[test]
fn conformance_goldens_match_the_no_corpus_output() {
    let temp = tempfile::tempdir().expect("empty workspace");
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin/tests/conformance");
    for verb in VERBS {
        let text = fs::read_to_string(directory.join(format!("{verb}.yaml"))).expect("golden");
        let golden: Value = serde_yaml::from_str(&text).expect("golden YAML");
        let cases = golden["tests"].as_array().expect("cases");
        let expected = cases[0]["expect"]["output"].clone();
        assert_eq!(
            panel(verb, temp.path(), &ArtifactHost::default()),
            expected,
            "{verb}"
        );
        let refused = serve(
            verb,
            cases[1]["input"].clone(),
            temp.path(),
            &ArtifactHost::default(),
        );
        assert_eq!(
            refused["error"]["code"], cases[1]["expect"]["error"]["code"],
            "{verb}"
        );
    }
}
