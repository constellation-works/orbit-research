use super::writer::{fixture, git};
use crate::{
    Error,
    corpus::Corpus,
    edit::{Assessment, Edit, OrbitLink},
    writer::{WriteMode, WriteOutcome},
};
use serde_json::json;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

/// A primary corpus with Q001 and two reserved R stubs, plus a linked run worktree.
pub(super) struct Run {
    pub(super) primary: TempDir,
    _parent: TempDir,
    pub(super) linked: PathBuf,
}

pub(super) fn run() -> Run {
    let primary = fixture();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::write(primary.path().join(directory).join(".gitkeep"), "").unwrap();
    }
    git(primary.path(), &["add", "."]);
    git(
        primary.path(),
        &["commit", "-q", "-m", "keep kind directories"],
    );
    let corpus = Corpus::open(primary.path()).unwrap();
    corpus
        .reserve("q", "Q", "A question", "Why?", vec![], vec![])
        .unwrap();
    for (key, title) in [("r1", "First study"), ("r2", "Second study")] {
        corpus
            .reserve(key, "R", title, "Question", vec![], vec![])
            .unwrap();
    }
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("run");
    git(
        primary.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    Run {
        primary,
        _parent: parent,
        linked,
    }
}

pub(super) fn blob(corpus: &Corpus, id: &str) -> String {
    corpus
        .snapshot()
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
        .git_blob
}

pub(super) fn result() -> Edit {
    Edit {
        body: Some("## Question\n\nWhy?\n\n## Method\n\nRan it.\n\n## Result\n\nControls failed.\n\n## Limitations\n\nOne run.\n\n## Next\n\nRepeat.".into()),
        status: Some("done".into()),
        orbit: Some(OrbitLink {
            task: Some("task-1".into()),
            run: Some("run-1".into()),
        }),
        manifest: Some(json!({"inputs": [{"name": "input.csv", "sha256": null}]})),
        ..Edit::default()
    }
}

#[test]
fn reserved_research_is_written_without_a_commit() {
    let run = run();
    let corpus = Corpus::open(&run.linked).unwrap();
    assert_eq!(corpus.write_mode().unwrap(), WriteMode::Worktree);
    assert_eq!(
        Corpus::open(run.primary.path())
            .unwrap()
            .write_mode()
            .unwrap(),
        WriteMode::Primary
    );
    let head = git(&run.linked, &["rev-parse", "HEAD"]);
    let old = blob(&corpus, "R001");

    let outcome = corpus.revise("R001", &old, &result()).unwrap();
    let WriteOutcome::Worktree(write) = &outcome else {
        panic!("linked worktree must write in worktree mode");
    };
    assert_eq!(serde_json::to_value(&outcome).unwrap()["mode"], "worktree");
    assert_eq!(
        write.files,
        [
            "research/R001-first-study/README.md",
            "research/R001-first-study/data/manifest.json"
        ]
    );
    assert_eq!(write.blob, blob(&corpus, "R001"));
    assert_eq!(git(&run.linked, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&run.linked, &["status", "--porcelain"]),
        "M research/R001-first-study/README.md\n M research/R001-first-study/data/manifest.json"
    );
    let record = corpus
        .snapshot()
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.id == "R001")
        .unwrap();
    assert_eq!(record.metadata["status"], "done");
    assert_eq!(
        record.metadata["orbit"],
        json!({"task": "task-1", "run": "run-1"})
    );
    assert!(git(run.primary.path(), &["status", "--porcelain"]).is_empty());

    // An identical retry adopts; a different edit under the stale blob refuses untouched.
    let retried = corpus.revise("R001", &old, &result()).unwrap();
    assert_eq!(
        serde_json::to_value(&retried).unwrap(),
        serde_json::to_value(&outcome).unwrap()
    );
    let written = fs::read(run.linked.join("research/R001-first-study/README.md")).unwrap();
    let mut other = result();
    other.status = Some("abandoned".into());
    let error = corpus.revise("R001", &old, &other).unwrap_err();
    assert!(matches!(error, Error::Conflict(_)), "{error}");
    assert_eq!(
        fs::read(run.linked.join("research/R001-first-study/README.md")).unwrap(),
        written
    );
}

#[test]
fn worktree_refuses_allocation_and_every_other_record() {
    let run = run();
    let corpus = Corpus::open(&run.linked).unwrap();
    let head = git(&run.linked, &["rev-parse", "HEAD"]);
    let refused = |result: crate::Result<()>| {
        let error = result.unwrap_err();
        assert!(matches!(error, Error::Refused(_)), "{error}");
    };

    refused(
        corpus
            .reserve("new-r", "R", "New study", "Question", vec![], vec![])
            .map(drop),
    );
    let question = blob(&corpus, "Q001");
    refused(
        corpus
            .revise_question("Q001", &question, Some("Edit"), Some("Body"), Some(vec![]))
            .map(drop),
    );
    let edit = Edit {
        title: Some("Edited".into()),
        ..Edit::default()
    };
    refused(corpus.revise("Q001", &question, &edit).map(drop));
    let assessment = Assessment {
        research: "R001".into(),
        revision: 1,
        verdict: "inconclusive".into(),
        strength: "anecdote".into(),
        note: None,
    };
    refused(
        corpus
            .assess("Q001", &question, &assessment, |_| Ok(()))
            .map(drop),
    );

    // A research record created in the worktree was never reserved.
    let stray = run.linked.join("research/R003-stray");
    fs::create_dir_all(stray.join("data")).unwrap();
    fs::write(
        stray.join("README.md"),
        "---\nid: R003\ntitle: Stray\nstatus: planned\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: []\n---\n\nStray.\n",
    )
    .unwrap();
    let stray_blob = blob(&corpus, "R003");
    refused(corpus.revise("R003", &stray_blob, &result()).map(drop));
    fs::remove_dir_all(&stray).unwrap();

    // The first write binds the worktree to its reserved R.
    corpus
        .revise("R001", &blob(&corpus, "R001"), &result())
        .unwrap();
    refused(
        corpus
            .revise("R002", &blob(&corpus, "R002"), &result())
            .map(drop),
    );

    assert_eq!(git(&run.linked, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&run.linked, &["status", "--porcelain"]),
        "M research/R001-first-study/README.md\n M research/R001-first-study/data/manifest.json"
    );
}

#[test]
fn a_worktree_write_that_changes_nothing_says_so() {
    let run = run();
    let corpus = Corpus::open(&run.linked).unwrap();
    let edit = result();
    let first = match corpus
        .revise("R001", &blob(&corpus, "R001"), &edit)
        .unwrap()
    {
        WriteOutcome::Worktree(write) => write,
        other => panic!("worktree mode expected: {other:?}"),
    };
    assert!(first.changed);
    assert!(
        serde_json::to_value(&first)
            .unwrap()
            .get("changed")
            .is_none()
    );
    let again = match corpus.revise("R001", &first.blob, &edit).unwrap() {
        WriteOutcome::Worktree(write) => write,
        other => panic!("worktree mode expected: {other:?}"),
    };
    assert!(!again.changed);
    assert_eq!(serde_json::to_value(&again).unwrap()["changed"], false);
    assert_eq!(again.blob, first.blob);
}
