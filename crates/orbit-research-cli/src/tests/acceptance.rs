//! The production acceptance lookup against injected readers and a fake
//! `orbit` executable. Neither needs a real Orbit installation or plugin.
use super::super::acceptance::{ArtifactReader, OrbitAcceptance, OrbitArtifacts};
use super::plugin::{FakeTaskHost, call_accept, delivered};
use orbit_research_core::{
    AcceptanceFailure, Error, Result,
    application::acceptance::{Acceptance, AcceptanceLookup},
};
use serde_json::{Value, json};
use std::process::Command;
use std::sync::Mutex;

fn stored() -> serde_json::Value {
    json!({
        "research_id": "R001",
        "commit": "c".repeat(40),
        "blob": "b".repeat(40),
        "run_id": "run-1",
        "artifact_digests": {"input.csv": "d".repeat(64)},
    })
}

/// A reader answering from one canned result and recording what was asked.
struct Canned {
    answer: Mutex<Option<Result<Option<String>>>>,
    asked: Mutex<Vec<(String, String)>>,
}

impl Canned {
    fn new(answer: Result<Option<String>>) -> Self {
        Self {
            answer: Mutex::new(Some(answer)),
            asked: Mutex::default(),
        }
    }
}

impl ArtifactReader for &Canned {
    fn read(&self, task: &str, path: &str) -> Result<Option<String>> {
        self.asked
            .lock()
            .expect("asked")
            .push((task.into(), path.into()));
        self.answer
            .lock()
            .expect("answer")
            .take()
            .expect("one read")
    }
}

fn lookup(canned: &Canned) -> Result<Option<Acceptance>> {
    OrbitAcceptance::new(canned).acceptance("R001", "task-1")
}

fn failure(result: Result<Option<Acceptance>>) -> AcceptanceFailure {
    match result.expect_err("a refusal") {
        Error::Acceptance(failure) => failure,
        other => panic!("expected an acceptance failure, got {other:?}"),
    }
}

#[test]
fn stored_artifact_decodes_into_the_acceptance_accept_wrote() {
    let canned = Canned::new(Ok(Some(stored().to_string())));
    let acceptance = lookup(&canned).expect("lookup").expect("acceptance");
    assert_eq!(acceptance.research_id, "R001");
    assert_eq!(acceptance.blob, "b".repeat(40));
    assert_eq!(acceptance.run_id, "run-1");
    assert_eq!(acceptance.artifact_digests["input.csv"], "d".repeat(64));
    assert_eq!(
        *canned.asked.lock().expect("asked"),
        [("task-1".to_owned(), "research-acceptance.json".to_owned())]
    );
}

#[test]
fn a_task_that_never_stored_acceptance_has_none() {
    assert!(lookup(&Canned::new(Ok(None))).expect("lookup").is_none());
}

#[test]
fn an_unreachable_orbit_is_a_typed_refusal_never_none() {
    let canned = Canned::new(Err(Error::Internal(
        "`orbit tool run orbit.task.show` failed: boom".into(),
    )));
    let failure = failure(lookup(&canned));
    assert!(
        matches!(&failure, AcceptanceFailure::Unreachable { research, task, reason }
            if research == "R001" && task == "task-1" && reason.contains("boom")),
        "{failure:?}"
    );
    assert!(failure.to_string().contains("ORBIT_BIN"), "{failure}");
}

#[test]
fn unreadable_artifacts_are_typed_refusals() {
    for text in [
        "not json".to_owned(),
        json!({"research_id": "R001"}).to_string(),
        json!({"research_id": 1, "commit": "c", "blob": "b", "run_id": "r"}).to_string(),
    ] {
        let failure = failure(lookup(&Canned::new(Ok(Some(text.clone())))));
        assert!(
            matches!(failure, AcceptanceFailure::Malformed { .. }),
            "{text}: {failure:?}"
        );
    }
}

#[cfg(unix)]
#[path = "../../tests/exec_support/mod.rs"]
mod exec_support;

#[cfg(unix)]
mod fake_orbit {
    use super::exec_support;
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// One shared script: its per-test canned outputs live under the working
    /// directory the adapter runs it in, so tests never rewrite the executable.
    fn script() -> &'static Path {
        static SCRIPT: OnceLock<PathBuf> = OnceLock::new();
        SCRIPT.get_or_init(|| {
            let dir = tempfile::tempdir().expect("script dir").keep();
            let path = dir.join("orbit");
            exec_support::install_executable(
                &path,
                "#!/bin/sh\nd=\"$PWD/.git/fake-orbit\"\nprintf '%s\\n' \"$3 $5\" >> \"$d/calls\"\npwd > \"$d/cwd\"\nif [ -f \"$d/$3.fail\" ]; then cat \"$d/$3.fail\" >&2; exit 1; fi\ncat \"$d/$3.out\"\n",
            );
            path
        })
    }

    struct Workspace(tempfile::TempDir);

    impl Workspace {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("workspace");
            fs::create_dir_all(temp.path().join(".git/fake-orbit")).expect("fake state");
            Self(temp)
        }

        fn state(&self, name: &str, contents: impl AsRef<[u8]>) {
            fs::write(self.0.path().join(".git/fake-orbit").join(name), contents)
                .expect("canned output");
        }

        fn lookup_with(&self, orbit: &str) -> Result<Option<Acceptance>> {
            OrbitAcceptance::new(OrbitArtifacts::new(orbit, self.0.path()))
                .acceptance("R001", "task-1")
        }

        fn lookup(&self) -> Result<Option<Acceptance>> {
            self.lookup_with(script().to_str().expect("UTF-8 path"))
        }

        fn calls(&self) -> String {
            fs::read_to_string(self.0.path().join(".git/fake-orbit/calls")).unwrap_or_default()
        }
    }

    fn listing() -> String {
        json!({"artifacts": [{"path": "research-acceptance.json"}]}).to_string()
    }

    #[test]
    fn fetches_the_artifact_through_orbit_from_the_corpus_checkout() {
        let workspace = Workspace::new();
        workspace.state("orbit.task.show.out", listing());
        workspace.state(
            "orbit.task.artifact.get.out",
            json!({"media_type": "application/json", "content": stored().to_string()}).to_string(),
        );
        let acceptance = workspace.lookup().expect("lookup").expect("acceptance");
        assert_eq!(acceptance.blob, "b".repeat(40));
        let calls = workspace.calls();
        assert!(
            calls.contains(r#"orbit.task.show {"fields":["artifacts"],"id":"task-1"}"#),
            "{calls}"
        );
        assert!(
            calls.contains(
                r#"orbit.task.artifact.get {"id":"task-1","path":"research-acceptance.json"}"#
            ),
            "{calls}"
        );
        let cwd = fs::read_to_string(workspace.0.path().join(".git/fake-orbit/cwd"))
            .expect("recorded cwd");
        assert_eq!(
            Path::new(cwd.trim()).canonicalize().expect("cwd"),
            workspace.0.path().canonicalize().expect("workspace")
        );
    }

    #[test]
    fn a_single_field_listing_is_a_bare_array() {
        // `orbit.task.show` with one requested field returns just that field.
        let workspace = Workspace::new();
        workspace.state(
            "orbit.task.show.out",
            json!([{"path": "research-acceptance.json", "size": 8}]).to_string(),
        );
        workspace.state(
            "orbit.task.artifact.get.out",
            json!({"content": stored().to_string()}).to_string(),
        );
        assert!(workspace.lookup().expect("lookup").is_some());
    }

    #[test]
    fn a_task_without_the_artifact_has_none_and_is_not_fetched() {
        let workspace = Workspace::new();
        workspace.state(
            "orbit.task.show.out",
            json!({"artifacts": [{"path": "other.json"}]}).to_string(),
        );
        assert!(workspace.lookup().expect("lookup").is_none());
        assert!(
            !workspace.calls().contains("artifact.get"),
            "{}",
            workspace.calls()
        );
    }

    #[test]
    fn orbit_failure_output_or_absence_refuses_as_unreachable() {
        let workspace = Workspace::new();
        workspace.state("orbit.task.show.fail", "workspace not registered");
        let refused = failure(workspace.lookup());
        assert!(
            matches!(&refused, AcceptanceFailure::Unreachable { reason, .. }
                if reason.contains("workspace not registered")),
            "{refused:?}"
        );

        let workspace = Workspace::new();
        workspace.state("orbit.task.show.out", "warning, not JSON");
        let refused = failure(workspace.lookup());
        assert!(
            matches!(refused, AcceptanceFailure::Unreachable { .. }),
            "{refused:?}"
        );

        let workspace = Workspace::new();
        let missing = workspace.0.path().join("no-such-orbit");
        let refused = failure(workspace.lookup_with(missing.to_str().expect("UTF-8 path")));
        assert!(
            matches!(refused, AcceptanceFailure::Unreachable { .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_fetched_artifact_that_is_not_the_accept_shape_is_malformed() {
        let workspace = Workspace::new();
        workspace.state("orbit.task.show.out", listing());
        workspace.state(
            "orbit.task.artifact.get.out",
            json!({"content": "{\"unexpected\":true}"}).to_string(),
        );
        let refused = failure(workspace.lookup());
        assert!(
            matches!(refused, AcceptanceFailure::Malformed { .. }),
            "{refused:?}"
        );

        let workspace = Workspace::new();
        workspace.state("orbit.task.show.out", listing());
        workspace.state(
            "orbit.task.artifact.get.out",
            json!({"content_base64": "e30="}).to_string(),
        );
        let refused = failure(workspace.lookup());
        assert!(
            matches!(&refused, AcceptanceFailure::Unreachable { reason, .. }
                if reason.contains("text content")),
            "{refused:?}"
        );
    }
}

/// Reads what the plugin's `accept` tool stored in its (in-memory) task host.
struct AcceptedByPlugin(std::sync::Arc<FakeTaskHost>);

impl ArtifactReader for AcceptedByPlugin {
    fn read(&self, task: &str, path: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .artifacts
            .lock()
            .expect("artifacts")
            .get(&(task.to_owned(), path.to_owned()))
            .map(Value::to_string))
    }
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Acceptance tests")
        .env("GIT_AUTHOR_EMAIL", "tests@example.invalid")
        .env("GIT_COMMITTER_NAME", "Acceptance tests")
        .env("GIT_COMMITTER_EMAIL", "tests@example.invalid")
        .output()
        .expect("run fixture Git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assess(app: &orbit_research_core::Application, note: &str) -> Result<Value> {
    let hypothesis = app
        .call("research.show", json!({"id": "H001"}))
        .expect("show H001");
    app.call(
        "research.assess",
        json!({
            "id": "H001",
            "expected_blob": hypothesis["git_blob"],
            "research": "R001",
            "revision": 1,
            "verdict": "inconclusive",
            "strength": "suggestive",
            "note": note,
        }),
    )
}

/// The real `accept` implementation and the real lookup share one artifact:
/// whatever `accept` stored is exactly what `assess` verifies, and a README
/// that changes afterwards is no longer the accepted one.
#[test]
fn assess_verifies_what_accept_stored_and_refuses_a_readme_edited_afterwards() {
    let delivered = delivered();
    delivered.merge_into_primary();
    let host = std::sync::Arc::new(FakeTaskHost::default());
    host.seed_task_state("task-1", "review", Some("run-1"));
    let accepted = call_accept(delivered.primary.path(), "task-1", "R001", host.as_ref());
    assert_eq!(accepted["output"]["recorded"], true, "{accepted:?}");

    let app = orbit_research_core::Application::local(delivered.primary.path())
        .expect("open primary")
        .with_acceptance(OrbitAcceptance::new(AcceptedByPlugin(host.clone())));
    app.call(
        "research.create",
        json!({"kind": "H", "title": "Claim", "request_key": "h1"}),
    )
    .expect("create H001");
    assess(&app, "Controls failed").expect("assess the accepted research");

    // Amend the delivered README after acceptance.
    let readme = delivered
        .primary
        .path()
        .join("research/R001-study/README.md");
    let text = std::fs::read_to_string(&readme).expect("README");
    std::fs::write(&readme, text.replace("Repeat.", "Repeat twice.")).expect("amend README");
    git(delivered.primary.path(), &["commit", "-qam", "Amend R001"]);
    let app = orbit_research_core::Application::local(delivered.primary.path())
        .expect("reopen primary")
        .with_acceptance(OrbitAcceptance::new(AcceptedByPlugin(host.clone())));
    let error = assess(&app, "Second look").expect_err("README no longer matches acceptance");
    assert!(
        matches!(&error, Error::Acceptance(AcceptanceFailure::StaleBlob { research, task, .. })
            if research == "R001" && task == "task-1"),
        "{error:?}"
    );
}
