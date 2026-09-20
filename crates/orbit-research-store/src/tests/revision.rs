use crate::corpus::Corpus;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    fs::write(root.join("questions/Q001-original.md"), "---\nid: Q001\ntitle: Original\nstatus: open\ntags: [initial]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []\n---\nQuestion text.\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
    git(root, &["config", "user.name", "Revision tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    temp
}

#[test]
fn valid_question_edit_updates_content_and_preserves_frozen_path() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    let revision = corpus
        .revise_question(
            "Q001",
            &old,
            "A changed title",
            "A changed question body.",
            vec!["updated".into()],
        )
        .unwrap();

    assert_eq!(revision.id, "Q001");
    assert_eq!(revision.path, "questions/Q001-original.md");
    assert_eq!(
        git(temp.path(), &["diff", "HEAD^", "--name-only"]),
        revision.path
    );
    let record = &corpus.snapshot().unwrap().records[0];
    assert_eq!(record.path, revision.path);
    assert_eq!(record.metadata["title"], "A changed title");
    assert_eq!(record.metadata["slug"], "original");
    assert_eq!(record.metadata["tags"], serde_json::json!(["updated"]));
    assert!(record.body.contains("A changed question body."));
}

#[test]
fn stale_blob_is_refused_before_writing() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    corpus
        .revise_question("Q001", &old, "First edit", "body", vec!["one".into()])
        .unwrap();
    let error = corpus
        .revise_question("Q001", &old, "Stale edit", "body", vec!["two".into()])
        .unwrap_err();
    assert!(error.to_string().contains("changed since it was opened"));
    let record = &corpus.snapshot().unwrap().records[0];
    assert_eq!(record.metadata["title"], "First edit");
    assert!(!git(temp.path(), &["status", "--porcelain"]).contains("Q001"));
}

#[test]
fn only_questions_are_editable() {
    let temp = fixture();
    fs::write(temp.path().join("hypotheses/H001-h.md"), "---\nid: H001\ntitle: H\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nrevision: 1\nassessments: []\n---\nH\n").unwrap();
    fs::write(temp.path().join("theories/T001-t.md"), "---\nid: T001\ntitle: T\nstatus: active\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nclaims: []\nsupersedes: []\n---\nT\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-q", "-m", "more records"]);
    let corpus = Corpus::open(temp.path()).unwrap();
    for id in ["H001", "T001"] {
        let error = corpus
            .revise_question(id, "unused", "Edit", "body", vec![])
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Only existing questions are editable"),
            "{id}: {error}"
        );
    }
}
