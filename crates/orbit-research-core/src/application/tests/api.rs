use super::super::{
    Operation,
    acceptance::{Acceptance, AcceptanceLookup},
};
use crate::{Application, Error, Result};
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
        "---\nid: R001\ntitle: Negative\nstatus: done\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: [H001]\norbit: {task: task-1, run: run-1}\n---\n\n## Question\n\nDoes it exist?\n\n## Method\n\nOne run with positive and negative controls.\n\n## Result\n\nThe run completed successfully, but both controls failed.\n\n## Limitations\n\nNo usable signal.\n\n## Next\n\nFix the controls.\n",
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

/// Fixture lookup: a successful run whose result was accepted.
struct Accepted(&'static str);

impl AcceptanceLookup for Accepted {
    fn acceptance(&self, research_id: &str) -> Result<Option<Acceptance>> {
        Ok((research_id == self.0).then(|| Acceptance {
            research_id: research_id.into(),
            commit: "0".repeat(40),
            blob: "0".repeat(40),
            run_id: "run-1".into(),
            artifact_digests: Default::default(),
        }))
    }
}

/// A lookup backed by a stored task artifact, as the plugin's `accept` tool
/// would persist and the CLI's `assess` path would fetch. Deserializing
/// straight into `Acceptance` is the "connection" between the two: `accept`
/// and `assess` share the same wire shape for `research-acceptance.json`.
struct FromStoredArtifact(Value);

impl AcceptanceLookup for FromStoredArtifact {
    fn acceptance(&self, research_id: &str) -> Result<Option<Acceptance>> {
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
        "blob": "0".repeat(40),
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

#[test]
fn assess_refuses_research_without_an_acceptance_record() {
    let temp = negative_result_corpus();
    let head = git(&root(&temp), &["rev-parse", "HEAD"]);
    for app in [
        Application::local(&root(&temp)).expect("default app"),
        Application::local(&root(&temp))
            .expect("app")
            .with_acceptance(Accepted("R999")),
    ] {
        let error = assess(&app, Some("inconclusive"), 1).unwrap_err();
        assert!(
            error.to_string().contains("R001 has no acceptance record"),
            "{error}"
        );
    }
    assert_eq!(git(&root(&temp), &["rev-parse", "HEAD"]), head);
    assert!(git(&root(&temp), &["status", "--porcelain"]).is_empty());
}

#[test]
fn negative_result_is_assessed_inconclusive_never_supported() {
    let temp = negative_result_corpus();
    let app = Application::local(&root(&temp))
        .expect("app")
        .with_acceptance(Accepted("R001"));

    // Execution success and acceptance supply no verdict: the author must give one.
    let error = assess(&app, None, 1).unwrap_err();
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
        .with_acceptance(Accepted("R001"));
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

    let error = assess(&app, Some("refutes"), 3).unwrap_err();
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
    let error = reserve("done", "primary").unwrap_err();
    assert!(error.to_string().contains("start as planned"), "{error}");
    let error = reserve("planned", "worktree").unwrap_err();
    assert!(matches!(error, Error::Refused(_)), "{error}");
    let reserved = reserve("planned", "primary").expect("reservation");
    assert_eq!(reserved["id"], "R002");
    assert_eq!(reserved["mode"], "primary");
}
