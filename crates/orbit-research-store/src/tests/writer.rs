use crate::corpus::Corpus;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
};
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

fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
    git(root, &["config", "user.name", "Writer tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    temp
}

fn reserve_q(corpus: &Corpus, key: &str, title: &str, body: &str) -> crate::writer::Reservation {
    corpus
        .reserve(key, "Q", title, body, vec!["capture".into()], vec![])
        .unwrap()
}

#[test]
fn q_capture_is_persisted_in_a_canonical_commit() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let parent = git(temp.path(), &["rev-parse", "HEAD"]);
    let reservation = reserve_q(&corpus, "capture-1", "A question", "What happened?");

    assert_eq!(reservation.id, "Q001");
    assert_eq!(reservation.path, "questions/Q001-a-question.md");
    assert_ne!(reservation.commit, parent);
    assert_eq!(reservation.commit, git(temp.path(), &["rev-parse", "HEAD"]));
    assert!(git(temp.path(), &["log", "-1", "--format=%B"]).contains("Orbit-Research-Request:"));
    let snapshot = corpus.snapshot().unwrap();
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].id, "Q001");
    assert_eq!(
        snapshot.records[0].body.trim_start(),
        "# Q001 — A question\n\n## The question\n\nWhat happened?\n"
    );
    assert!(git(temp.path(), &["status", "--porcelain"]).is_empty());
}

#[test]
fn retry_returns_same_reservation_and_different_content_is_refused() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = reserve_q(&corpus, "retry-key", "Retry me", "first");
    let second = reserve_q(&corpus, "retry-key", "Retry me", "first");
    assert_eq!(first.id, second.id);
    assert_eq!(first.path, second.path);
    assert_eq!(first.commit, second.commit);
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");

    let error = corpus
        .reserve(
            "retry-key",
            "Q",
            "Retry me",
            "changed",
            vec!["capture".into()],
            vec![],
        )
        .unwrap_err();
    assert!(error.to_string().contains("different content"));
}

#[test]
fn sequential_requests_get_distinct_ids_and_commits() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = reserve_q(&corpus, "one", "First", "one");
    let second = reserve_q(&corpus, "two", "Second", "two");
    assert_eq!(first.id, "Q001");
    assert_eq!(second.id, "Q002");
    assert_ne!(first.commit, second.commit);
    assert_eq!(corpus.snapshot().unwrap().records.len(), 2);
}

#[test]
fn concurrent_requests_are_serialized_and_get_distinct_ids() {
    let temp = fixture();
    let root = Arc::new(PathBuf::from(temp.path()));
    let mut workers = Vec::new();
    for (key, title) in [("parallel-a", "Parallel A"), ("parallel-b", "Parallel B")] {
        let root = Arc::clone(&root);
        workers.push(thread::spawn(move || {
            let corpus = Corpus::open(&root).unwrap();
            reserve_q(&corpus, key, title, "parallel body")
        }));
    }
    let first = workers.remove(0).join().unwrap();
    let second = workers.remove(0).join().unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(
        [first.id.as_str(), second.id.as_str()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        ["Q001", "Q002"].into_iter().collect()
    );
    assert_eq!(
        Corpus::open(&root)
            .unwrap()
            .snapshot()
            .unwrap()
            .records
            .len(),
        2
    );
}

#[test]
fn dirty_checkout_is_rejected_without_clobbering_existing_edits() {
    let temp = fixture();
    fs::write(temp.path().join("notes.txt"), "baseline\n").unwrap();
    git(temp.path(), &["add", "notes.txt"]);
    git(temp.path(), &["commit", "-q", "-m", "notes"]);
    fs::write(temp.path().join("notes.txt"), "Daniel's existing edit\n").unwrap();
    let before = fs::read_to_string(temp.path().join("notes.txt")).unwrap();

    let corpus = Corpus::open(temp.path()).unwrap();
    let error = corpus
        .reserve("dirty", "Q", "Should fail", "body", vec![], vec![])
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("clean corpus integration checkout")
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("notes.txt")).unwrap(),
        before
    );
    assert_eq!(corpus.snapshot().unwrap().records.len(), 0);
}

#[test]
fn r_reservation_creates_stub_manifest_and_canonical_layout() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let reservation = corpus
        .reserve(
            "research-1",
            "R",
            "A study",
            "Question under study",
            vec!["lab".into()],
            vec![],
        )
        .unwrap();
    assert_eq!(reservation.id, "R001");
    let root = temp.path().join("research/R001-a-study");
    assert_eq!(
        fs::read_to_string(root.join("data/manifest.json")).unwrap(),
        "{\"inputs\":[]}\n"
    );
    assert!(root.join("code").is_dir());
    assert!(root.join("artifacts").is_dir());
    let snapshot = corpus.snapshot().unwrap();
    assert_eq!(snapshot.records[0].kind, "R");
    assert_eq!(snapshot.records[0].id, "R001");
}

fn revision_fixture() -> TempDir {
    let temp = fixture();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::write(temp.path().join(directory).join(".gitkeep"), "").unwrap();
    }
    fs::write(
        temp.path().join("questions/Q001-original.md"),
        "---\nid: Q001\ntitle: Original\nstatus: open\ntags: [initial]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []\n---\nQuestion text.\n",
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-q", "-m", "question fixture"]);
    temp
}

#[test]
fn valid_question_edit_updates_content_and_preserves_frozen_path() {
    let temp = revision_fixture();
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
    let temp = revision_fixture();
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
    let temp = revision_fixture();
    fs::write(
        temp.path().join("hypotheses/H001-h.md"),
        "---\nid: H001\ntitle: H\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nrevision: 1\nassessments: []\n---\nH\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("theories/T001-t.md"),
        "---\nid: T001\ntitle: T\nstatus: active\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nclaims: []\nsupersedes: []\n---\nT\n",
    )
    .unwrap();
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

#[test]
fn linked_worktree_cannot_revise_canonical_question() {
    let temp = revision_fixture();
    let linked_parent = tempfile::tempdir().expect("worktree parent");
    let linked = linked_parent.path().join("linked");
    git(
        temp.path(),
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().expect("path"),
        ],
    );
    let corpus = Corpus::open(&linked).expect("linked corpus");
    let blob = corpus.snapshot().expect("snapshot").records[0]
        .git_blob
        .clone();
    let error = corpus
        .revise_question("Q001", &blob, "Changed", "Text", vec![])
        .expect_err("linked revision must refuse");
    assert!(error.to_string().contains("primary integration checkout"));
    assert!(git(&linked, &["status", "--porcelain"]).is_empty());
}

#[test]
fn hypothesis_revision_is_a_plain_yaml_integer() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).expect("corpus");
    let reservation = corpus
        .reserve(
            "hypothesis",
            "H",
            "A hypothesis",
            "Statement",
            vec![],
            vec![],
        )
        .expect("reserve hypothesis");
    let text = fs::read_to_string(temp.path().join(reservation.path)).expect("record");
    let frontmatter = text.split("---").nth(1).expect("frontmatter");
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).expect("plain YAML");
    assert_eq!(yaml["revision"].as_u64(), Some(1));
    assert!(!text.contains("$serde_json"));
}
