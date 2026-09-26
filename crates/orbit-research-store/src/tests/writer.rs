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

#[cfg(unix)]
fn install_failing_pre_commit(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let hooks = root.join(".git/test-hooks");
    fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    let mut permissions = fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).unwrap();
    git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    hook
}

#[cfg(unix)]
#[test]
fn failed_question_commit_is_inspectable_and_identical_retry_succeeds() {
    let temp = revision_fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    let hook = install_failing_pre_commit(temp.path());

    corpus
        .revise_question(
            "Q001",
            &old,
            "Retryable edit",
            "Body with trailing spaces.  ",
            vec!["retry".into()],
        )
        .unwrap_err();
    let edited = fs::read(temp.path().join("questions/Q001-original.md")).unwrap();
    assert!(String::from_utf8_lossy(&edited).contains("Retryable edit"));
    assert_eq!(
        git(temp.path(), &["diff", "--cached", "--name-only"]),
        "questions/Q001-original.md"
    );

    fs::remove_file(hook).unwrap();
    let revision = corpus
        .revise_question(
            "Q001",
            &old,
            "Retryable edit",
            "Body with trailing spaces.  ",
            vec!["retry".into()],
        )
        .unwrap();
    assert_eq!(revision.commit, git(temp.path(), &["rev-parse", "HEAD"]));
    assert_eq!(fs::read(temp.path().join(&revision.path)).unwrap(), edited);
    assert!(git(temp.path(), &["status", "--porcelain"]).is_empty());
}

#[cfg(unix)]
#[test]
fn conflicting_edit_after_failed_question_commit_is_preserved_and_refused() {
    let temp = revision_fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    let hook = install_failing_pre_commit(temp.path());
    corpus
        .revise_question("Q001", &old, "Pending edit", "body", vec!["retry".into()])
        .unwrap_err();
    fs::remove_file(hook).unwrap();

    let path = temp.path().join("questions/Q001-original.md");
    let conflicting = b"external edit with exact bytes  \n\n";
    fs::write(&path, conflicting).unwrap();
    let error = corpus
        .revise_question("Q001", &old, "Pending edit", "body", vec!["retry".into()])
        .unwrap_err();
    assert!(error.to_string().contains("conflicting"));
    assert_eq!(fs::read(path).unwrap(), conflicting);
}

fn clear_reservation_result(root: &Path) -> PathBuf {
    let state = root.join(".git/orbit-research-writer");
    let intent_path = fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let mut intent: serde_json::Value =
        serde_json::from_slice(&fs::read(&intent_path).unwrap()).unwrap();
    intent["reservation"] = serde_json::Value::Null;
    fs::write(&intent_path, serde_json::to_vec(&intent).unwrap()).unwrap();
    intent_path
}

#[test]
fn retry_recovers_question_committed_before_receipt_was_saved() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = reserve_q(&corpus, "recover-q-success", "Recovery", "exact body");
    clear_reservation_result(temp.path());

    let recovered = reserve_q(&corpus, "recover-q-success", "Recovery", "exact body");
    assert_eq!(recovered.id, first.id);
    assert_eq!(recovered.path, first.path);
    assert_eq!(recovered.commit, first.commit);
    assert_eq!(recovered.request_digest, first.request_digest);
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn retry_recovers_research_committed_before_receipt_was_saved() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = corpus
        .reserve(
            "recover-r-success",
            "R",
            "Recovery study",
            "question",
            vec!["recovery".into()],
            vec![],
        )
        .unwrap();
    clear_reservation_result(temp.path());

    let recovered = corpus
        .reserve(
            "recover-r-success",
            "R",
            "Recovery study",
            "question",
            vec!["recovery".into()],
            vec![],
        )
        .unwrap();
    assert_eq!(recovered.id, first.id);
    assert_eq!(recovered.path, first.path);
    assert_eq!(recovered.commit, first.commit);
    assert_eq!(recovered.request_digest, first.request_digest);
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn portable_reservation_and_revision_retry_after_receipt_loss() {
    let research = fixture();
    let corpus = Corpus::open(research.path()).unwrap();
    let first = corpus
        .reserve(
            "portable-research",
            "R",
            "Portable study",
            "question",
            vec![],
            vec![],
        )
        .unwrap();
    assert!(
        research
            .path()
            .join("research/R001-portable-study/data/manifest.json")
            .is_file()
    );
    clear_reservation_result(research.path());
    let retried = corpus
        .reserve(
            "portable-research",
            "R",
            "Portable study",
            "question",
            vec![],
            vec![],
        )
        .unwrap();
    assert_eq!(retried.commit, first.commit);
    assert_eq!(git(research.path(), &["rev-list", "--count", "HEAD"]), "2");

    let questions = revision_fixture();
    let corpus = Corpus::open(questions.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    let first = corpus
        .revise_question("Q001", &old, "Portable edit", "new body", vec![])
        .unwrap();
    clear_reservation_result(questions.path());
    let retried = corpus
        .revise_question("Q001", &old, "Portable edit", "new body", vec![])
        .unwrap();
    assert_eq!(retried.commit, first.commit);
    assert_eq!(git(questions.path(), &["rev-list", "--count", "HEAD"]), "3");
    assert!(
        fs::read_to_string(questions.path().join(&first.path))
            .unwrap()
            .contains("new body")
    );
}

#[test]
fn reservation_recovery_compares_exact_record_bytes() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let reservation = reserve_q(&corpus, "recover-q", "Recovery", "exact body");
    clear_reservation_result(temp.path());

    let path = temp.path().join(&reservation.path);
    let mut changed = fs::read(&path).unwrap();
    changed.extend_from_slice(b"  \n");
    fs::write(&path, &changed).unwrap();
    git(temp.path(), &["add", "--", &reservation.path]);
    git(temp.path(), &["commit", "--amend", "--no-edit", "-q"]);

    let error = corpus
        .reserve(
            "recover-q",
            "Q",
            "Recovery",
            "exact body",
            vec!["capture".into()],
            vec![],
        )
        .unwrap_err();
    assert!(error.to_string().contains("content differs"));
    assert_eq!(fs::read(path).unwrap(), changed);
}

#[test]
fn research_reservation_recovery_validates_the_exact_manifest_bytes() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let reservation = corpus
        .reserve(
            "recover-r",
            "R",
            "Recovery study",
            "question",
            vec!["recovery".into()],
            vec![],
        )
        .unwrap();
    clear_reservation_result(temp.path());

    let directory = Path::new(&reservation.path).parent().unwrap();
    let manifest_relative = directory.join("data/manifest.json");
    let manifest = temp.path().join(&manifest_relative);
    let changed = b"{\"inputs\":[]}  \n";
    fs::write(&manifest, changed).unwrap();
    git(
        temp.path(),
        &["add", "--", manifest_relative.to_str().unwrap()],
    );
    git(temp.path(), &["commit", "--amend", "--no-edit", "-q"]);

    let error = corpus
        .reserve(
            "recover-r",
            "R",
            "Recovery study",
            "question",
            vec!["recovery".into()],
            vec![],
        )
        .unwrap_err();
    assert!(error.to_string().contains("manifest"));
    assert_eq!(fs::read(manifest).unwrap(), changed);
}

#[cfg(unix)]
#[test]
fn retry_preserves_conflicting_index_even_when_working_file_matches_intent() {
    let temp = revision_fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let old = corpus.snapshot().unwrap().records[0].git_blob.clone();
    let hook = install_failing_pre_commit(temp.path());
    corpus
        .revise_question("Q001", &old, "Pending edit", "body", vec![])
        .unwrap_err();
    fs::remove_file(hook).unwrap();
    let relative = "questions/Q001-original.md";
    let path = temp.path().join(relative);
    let intended = fs::read(&path).unwrap();
    fs::write(&path, "external staged edit\n").unwrap();
    git(temp.path(), &["add", "--", relative]);
    fs::write(&path, intended).unwrap();
    let error = corpus
        .revise_question("Q001", &old, "Pending edit", "body", vec![])
        .unwrap_err();
    assert!(error.to_string().contains("Staged path"), "{error}");
    assert_eq!(
        git(temp.path(), &["show", ":questions/Q001-original.md"]),
        "external staged edit"
    );
}

#[test]
fn older_research_intent_without_file_list_recovers_original_receipt() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = corpus
        .reserve("legacy-r", "R", "Legacy study", "question", vec![], vec![])
        .unwrap();
    let intent_path = clear_reservation_result(temp.path());
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&intent_path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("files");
    fs::write(&intent_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let retry = corpus
        .reserve("legacy-r", "R", "Legacy study", "question", vec![], vec![])
        .unwrap();
    assert_eq!(retry.commit, first.commit);
    assert_eq!(retry.id, first.id);
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn malformed_intent_is_reported_and_preserved_without_reallocation() {
    let temp = fixture();
    let corpus = Corpus::open(temp.path()).unwrap();
    let first = reserve_q(&corpus, "damaged", "Question", "body");
    let path = clear_reservation_result(temp.path());
    fs::write(&path, b"{\"request_digest\":").unwrap();
    let error = corpus
        .reserve(
            "damaged",
            "Q",
            "Question",
            "body",
            vec!["capture".into()],
            vec![],
        )
        .unwrap_err();
    assert!(error.to_string().contains("Incomplete write intent"));
    assert!(error.to_string().contains(path.to_str().unwrap()));
    assert_eq!(fs::read(&path).unwrap(), b"{\"request_digest\":");
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), first.commit);
}
