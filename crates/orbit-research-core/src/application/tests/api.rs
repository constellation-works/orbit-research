use super::super::{
    Operation,
    acceptance::{Acceptance, AcceptanceLookup},
};
use crate::{AcceptanceFailure, Application, Error, Result};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../tests/fixtures/schema.json");

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git should be installed");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A hypothesis and a delivered negative result: the run succeeded, its controls failed.
fn negative_result_corpus() -> TempDir {
    negative_result_corpus_with("orbit: {task: task-1, run: run-1}\n")
}

fn negative_result_corpus_with(orbit: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("corpus");
    fs::create_dir_all(root.join("_scripts")).expect("corpus dir");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("schema");
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).expect("kind dir");
        fs::write(root.join(directory).join(".gitkeep"), "").expect("keep");
    }
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "tests@example.invalid"]);
    git(&root, &["config", "user.name", "Core tests"]);
    fs::write(
        root.join("hypotheses/H001-claim.md"),
        "---\nid: H001\ntitle: Claim\nstatus: open\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nrevision: 1\nassessments: []\n---\n\n## The claim\n\nThe effect exists.\n",
    )
    .expect("hypothesis");
    let research = root.join("research/R001-negative");
    fs::create_dir_all(research.join("data")).expect("research dir");
    fs::write(
        research.join("README.md"),
        format!("---\nid: R001\ntitle: Negative\nstatus: done\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: [H001]\n{orbit}---\n\n## Question\n\nDoes it exist?\n\n## Method\n\nOne run with positive and negative controls.\n\n## Result\n\nThe run completed successfully, but both controls failed.\n\n## Limitations\n\nNo usable signal.\n\n## Next\n\nFix the controls.\n"),
    )
    .expect("research");
    fs::write(research.join("data/manifest.json"), "{\"inputs\":[]}\n").expect("manifest");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "negative result"]);
    temp
}

fn root(temp: &TempDir) -> std::path::PathBuf {
    temp.path().join("corpus")
}

/// The accepted README's blob, as `accept` stores it: the R README at HEAD.
fn readme_blob(temp: &TempDir) -> String {
    git(
        &root(temp),
        &["rev-parse", "HEAD:research/R001-negative/README.md"],
    )
}

fn acceptance(research: &str, blob: &str) -> Acceptance {
    Acceptance {
        research_id: research.into(),
        commit: "0".repeat(40),
        blob: blob.into(),
        run_id: "run-1".into(),
        artifact_digests: Default::default(),
    }
}

/// Fixture lookup answering with a fixed result and recording each query.
struct Lookup {
    answer: Box<dyn Fn() -> Result<Option<Acceptance>>>,
    queries: std::sync::Mutex<Vec<(String, String)>>,
}

impl Lookup {
    fn new(answer: impl Fn() -> Result<Option<Acceptance>> + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            queries: Default::default(),
        }
    }

    fn accepting(acceptance: Acceptance) -> Self {
        Self::new(move || Ok(Some(acceptance.clone())))
    }
}

impl AcceptanceLookup for Lookup {
    fn acceptance(&self, research_id: &str, task: &str) -> Result<Option<Acceptance>> {
        self.queries
            .lock()
            .expect("queries")
            .push((research_id.into(), task.into()));
        (self.answer)()
    }
}

impl AcceptanceLookup for std::rc::Rc<Lookup> {
    fn acceptance(&self, research_id: &str, task: &str) -> Result<Option<Acceptance>> {
        self.as_ref().acceptance(research_id, task)
    }
}

/// A lookup backed by a stored task artifact, as the plugin's `accept` tool
/// would persist and the CLI's `assess` path would fetch. Deserializing
/// straight into `Acceptance` is the "connection" between the two: `accept`
/// and `assess` share the same wire shape for `research-acceptance.json`.
struct FromStoredArtifact(Value);

impl AcceptanceLookup for FromStoredArtifact {
    fn acceptance(&self, research_id: &str, _: &str) -> Result<Option<Acceptance>> {
        let acceptance: Acceptance =
            serde_json::from_value(self.0.clone()).expect("valid research-acceptance.json");
        Ok((acceptance.research_id == research_id).then_some(acceptance))
    }
}

#[test]
fn assess_succeeds_once_the_lookup_deserializes_accepts_stored_artifact_shape() {
    let temp = negative_result_corpus();
    let artifact = json!({
        "research_id": "R001",
        "commit": "0".repeat(40),
        "blob": readme_blob(&temp),
        "run_id": "run-1",
        "artifact_digests": {"input.csv": "0".repeat(64)},
    });
    let app = Application::local(&root(&temp))
        .expect("app")
        .with_acceptance(FromStoredArtifact(artifact));
    assess(&app, Some("inconclusive"), 1)
        .expect("assess succeeds when the task carries research-acceptance.json");
}

fn hypothesis(app: &Application) -> Value {
    app.execute(Operation::Show, json!({"id": "H001"}))
        .expect("show H001")
}

fn assess(app: &Application, verdict: Option<&str>, revision: u64) -> Result<Value> {
    let blob = hypothesis(app)["git_blob"].clone();
    let mut input = json!({
        "id": "H001",
        "expected_blob": blob,
        "research": "R001",
        "revision": revision,
        "strength": "suggestive",
        "note": "Controls failed; see R001 Limitations",
    });
    if let Some(verdict) = verdict {
        input["verdict"] = json!(verdict);
    }
    app.execute(Operation::Assess, input)
}

/// Assess against `app` and require the refusal to leave HEAD and the tree untouched.
fn assess_refused(temp: &TempDir, app: &Application) -> AcceptanceFailure {
    let head = git(&root(temp), &["rev-parse", "HEAD"]);
    let error = assess(app, Some("inconclusive"), 1)
        .expect_err("assessment requires verifiable acceptance");
    assert_eq!(git(&root(temp), &["rev-parse", "HEAD"]), head);
    assert!(git(&root(temp), &["status", "--porcelain"]).is_empty());
    match error {
        Error::Acceptance(failure) => failure,
        other => panic!("expected an acceptance refusal, got {other:?}"),
    }
}

#[test]
fn assess_asks_the_lookup_for_the_records_own_task_and_accepts_a_matching_blob() {
    let temp = negative_result_corpus();
    let lookup = std::rc::Rc::new(Lookup::accepting(acceptance("R001", &readme_blob(&temp))));
    let app = Application::local(&root(&temp))
        .expect("app")
        .with_acceptance(lookup.clone());
    assess(&app, Some("inconclusive"), 1).expect("matching acceptance");
    assert_eq!(
        *lookup.queries.lock().expect("queries"),
        [("R001".to_owned(), "task-1".to_owned())]
    );
}

#[test]
fn assess_refuses_the_default_lookup_as_missing_acceptance() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp)).expect("default app");
    let failure = assess_refused(&temp, &app);
    assert!(
        matches!(&failure, AcceptanceFailure::Missing { research, task }
            if research == "R001" && task == "task-1"),
        "{failure:?}"
    );
    assert!(failure.to_string().contains("`accept`"), "{failure}");
}

#[test]
fn assess_refuses_every_unverifiable_acceptance_without_writing() {
    let temp = negative_result_corpus();
    let blob = readme_blob(&temp);
    let app = |lookup: Lookup| {
        Application::local(&root(&temp))
            .expect("app")
            .with_acceptance(std::rc::Rc::new(lookup))
    };

    let failure = assess_refused(&temp, &app(Lookup::new(|| Ok(None))));
    assert!(
        matches!(failure, AcceptanceFailure::Missing { .. }),
        "{failure:?}"
    );

    let failure = assess_refused(&temp, &app(Lookup::accepting(acceptance("R999", &blob))));
    assert!(
        matches!(&failure, AcceptanceFailure::WrongResearch { found, .. } if found == "R999"),
        "{failure:?}"
    );

    let stale = "1".repeat(40);
    let failure = assess_refused(&temp, &app(Lookup::accepting(acceptance("R001", &stale))));
    assert!(
        matches!(&failure, AcceptanceFailure::StaleBlob { accepted, current, .. }
            if *accepted == stale && *current == blob),
        "{failure:?}"
    );

    let unreachable: fn() -> AcceptanceFailure = || AcceptanceFailure::Unreachable {
        research: "R001".into(),
        task: "task-1".into(),
        reason: "orbit not found".into(),
    };
    let malformed: fn() -> AcceptanceFailure = || AcceptanceFailure::Malformed {
        research: "R001".into(),
        task: "task-1".into(),
        reason: "bad json".into(),
    };
    let failure = assess_refused(&temp, &app(Lookup::new(move || Err(unreachable().into()))));
    assert!(
        matches!(failure, AcceptanceFailure::Unreachable { .. }),
        "{failure:?}"
    );
    let failure = assess_refused(&temp, &app(Lookup::new(move || Err(malformed().into()))));
    assert!(
        matches!(failure, AcceptanceFailure::Malformed { .. }),
        "{failure:?}"
    );
}

#[test]
fn assess_refuses_a_research_record_with_no_task() {
    for orbit in [
        "",
        "orbit: {run: run-1}\n",
        "orbit: {task: '', run: run-1}\n",
    ] {
        let temp = negative_result_corpus_with(orbit);
        let lookup = std::rc::Rc::new(Lookup::accepting(acceptance("R001", &readme_blob(&temp))));
        let app = Application::local(&root(&temp))
            .expect("app")
            .with_acceptance(lookup.clone());
        let failure = assess_refused(&temp, &app);
        assert!(
            matches!(&failure, AcceptanceFailure::NoTask { research } if research == "R001"),
            "{orbit:?}: {failure:?}"
        );
        assert!(
            lookup.queries.lock().expect("queries").is_empty(),
            "no task, so nothing to look up"
        );
    }
}

#[test]
fn negative_result_is_assessed_inconclusive_never_supported() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp))
        .expect("app")
        .with_acceptance(Lookup::accepting(acceptance("R001", &readme_blob(&temp))));

    // Execution success and acceptance supply no verdict: the author must give one.
    let error = assess(&app, None, 1).expect_err("assessment requires an explicit verdict");
    assert!(matches!(error, Error::InvalidInput(_)), "{error}");
    assert!(error.to_string().contains("verdict"), "{error}");

    let outcome = assess(&app, Some("inconclusive"), 1).expect("assess");
    assert_eq!(outcome["mode"], "primary");
    let metadata = &hypothesis(&app)["metadata"];
    assert_eq!(metadata["status"], "inconclusive");
    let entries = metadata["assessments"].as_array().expect("assessments");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["verdict"], "inconclusive");
    assert_eq!(entries[0]["research"], "R001");
    assert_eq!(entries[0]["revision"], 1);
    assert!(
        !serde_json::to_string(metadata)
            .expect("metadata JSON")
            .contains("support")
    );
}

#[test]
fn revised_hypothesis_keeps_earlier_assessments_on_their_revision() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp))
        .expect("app")
        .with_acceptance(Lookup::accepting(acceptance("R001", &readme_blob(&temp))));
    assess(&app, Some("inconclusive"), 1).expect("first assessment");
    let revised = app
        .execute(
            Operation::Revise,
            json!({
                "id": "H001",
                "expected_blob": hypothesis(&app)["git_blob"],
                "body": "## The claim\n\nThe effect exists above the control floor.",
            }),
        )
        .expect("revise");
    assert_eq!(revised["mode"], "primary");
    let metadata = &hypothesis(&app)["metadata"];
    assert_eq!(metadata["revision"], 2);
    assert_eq!(metadata["assessments"][0]["revision"], 1);

    let error = assess(&app, Some("refutes"), 3)
        .expect_err("assessment cannot cite an unknown hypothesis revision");
    assert!(error.to_string().contains("has no revision 3"), "{error}");
    assess(&app, Some("refutes"), 2).expect("assess revision 2");
    let metadata = &hypothesis(&app)["metadata"];
    assert_eq!(metadata["assessments"][0]["verdict"], "inconclusive");
    assert_eq!(metadata["assessments"][1]["verdict"], "refutes");
    assert_eq!(metadata["assessments"][1]["revision"], 2);
    assert_eq!(metadata["status"], "refuted");
}

#[test]
fn capture_needs_only_text_and_tags() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp)).expect("app");
    let input = json!({"text": "Does the effect survive better controls?\nContext line.", "tags": ["inbox"]});
    let captured = app
        .execute(Operation::Capture, input.clone())
        .expect("capture");
    assert_eq!(captured["mode"], "primary");
    assert_eq!(captured["id"], "Q001");
    let question = app
        .execute(Operation::Show, json!({"id": "Q001"}))
        .expect("show Q001");
    assert_eq!(
        question["metadata"]["title"],
        "Does the effect survive better controls?"
    );
    assert_eq!(question["metadata"]["tags"], json!(["inbox"]));
    // An identical capture adopts the first one instead of allocating again.
    let again = app.execute(Operation::Capture, input).expect("retry");
    assert_eq!(again["commit"], captured["commit"]);
}

#[test]
fn create_status_names_only_the_initial_status_and_mode_assertions_hold() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp)).expect("app");
    let reserve = |status: &str, mode: &str| {
        app.execute(
            Operation::Create,
            json!({"request_key": format!("r-{status}-{mode}"), "kind": "R", "title": "Study", "status": status, "mode": mode}),
        )
    };
    let error =
        reserve("done", "primary").expect_err("new research must start at its initial status");
    assert!(error.to_string().contains("start as planned"), "{error}");
    let error =
        reserve("planned", "worktree").expect_err("ID reservation requires the primary checkout");
    assert!(matches!(error, Error::Refused(_)), "{error}");
    let reserved = reserve("planned", "primary").expect("reservation");
    assert_eq!(reserved["id"], "R002");
    assert_eq!(reserved["mode"], "primary");
}
