use crate::corpus::Corpus;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn command(root: &Path, args: &[&str]) -> String {
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

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn record(root: &Path, relative: &str, frontmatter: &str, body: &str) {
    write(root, relative, &format!("---\n{frontmatter}\n---\n{body}"));
}

fn start_fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    temp
}

fn finish_fixture(temp: &TempDir) {
    let root = temp.path();
    command(root, &["init", "-q"]);
    command(root, &["config", "user.email", "tests@example.invalid"]);
    command(root, &["config", "user.name", "Corpus tests"]);
    command(root, &["add", "."]);
    command(root, &["commit", "-q", "-m", "fixture"]);
}

fn canonical_fixture() -> TempDir {
    let temp = start_fixture();
    let root = temp.path();
    record(
        root,
        "questions/Q001-why.md",
        "id: Q001\ntitle: Why\nstatus: answered\ntags: [logic, evidence]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-02\nanswered_by: [H001, R001]",
        "Question body.",
    );
    record(
        root,
        "hypotheses/H001-answer.md",
        "id: H001\ntitle: Answer\nstatus: supported\ntags: [evidence]\nderived_from: [Q001]\ncreated: 2026-01-01\nupdated: 2026-01-02\nrevision: 1\nassessments: []",
        "Hypothesis body.",
    );
    record(
        root,
        "theories/T001-model.md",
        "id: T001\ntitle: Model\nstatus: active\ntags: [logic]\nderived_from: [H001]\ncreated: 2026-01-01\nupdated: 2026-01-02\nclaims: [H001]\nsupersedes: []",
        "Theory body.",
    );
    record(
        root,
        "research/R001-run/README.md",
        "id: R001\ntitle: Run\nstatus: done\ntags: [measurement]\nderived_from: [Q001]\ncreated: 2026-01-01\nupdated: 2026-01-02\ntests: [H001]",
        "Research body.",
    );
    finish_fixture(&temp);
    temp
}

#[test]
fn reads_all_record_kinds_and_content_provenance() {
    let temp = canonical_fixture();
    let snapshot = Corpus::open(temp.path()).unwrap().snapshot().unwrap();

    assert_eq!(snapshot.records.len(), 4);
    assert_eq!(
        snapshot
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["H001", "Q001", "R001", "T001"]
    );
    assert_eq!(snapshot.tags, ["evidence", "logic", "measurement"]);
    assert_eq!(
        snapshot.revision,
        command(temp.path(), &["rev-parse", "HEAD"])
    );

    let question = snapshot
        .records
        .iter()
        .find(|record| record.id == "Q001")
        .unwrap();
    assert_eq!(question.kind, "Q");
    assert_eq!(question.path, "questions/Q001-why.md");
    assert_eq!(question.metadata["title"], "Why");
    assert_eq!(question.body, "Question body.");
    assert_eq!(question.content_sha256.len(), 64);
    assert_eq!(
        question.git_blob,
        command(temp.path(), &["hash-object", "--", "questions/Q001-why.md"])
    );

    let research = snapshot
        .records
        .iter()
        .find(|record| record.id == "R001")
        .unwrap();
    assert_eq!(research.kind, "R");
    assert_eq!(research.path, "research/R001-run/README.md");
    assert_eq!(research.body, "Research body.");
}

#[test]
fn rejects_duplicate_record_ids() {
    let temp = start_fixture();
    record(
        temp.path(),
        "questions/Q001-first.md",
        "id: Q001\ntitle: First\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
        "one",
    );
    record(
        temp.path(),
        "questions/Q001-second.md",
        "id: Q001\ntitle: Second\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
        "two",
    );
    finish_fixture(&temp);
    let error = Corpus::open(temp.path()).unwrap().snapshot().unwrap_err();
    assert!(error.to_string().contains("Duplicate record ID: Q001"));
}

#[test]
fn rejects_missing_references() {
    let temp = start_fixture();
    record(
        temp.path(),
        "questions/Q001-missing.md",
        "id: Q001\ntitle: Missing\nstatus: answered\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: [R999]",
        "body",
    );
    finish_fixture(&temp);
    let error = Corpus::open(temp.path()).unwrap().snapshot().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Q001 answered_by references missing R999")
    );
}

#[test]
fn rejects_lineage_cycles() {
    let temp = start_fixture();
    record(
        temp.path(),
        "questions/Q001-one.md",
        "id: Q001\ntitle: One\nstatus: open\ntags: [x]\nderived_from: [Q002]\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
        "one",
    );
    record(
        temp.path(),
        "questions/Q002-two.md",
        "id: Q002\ntitle: Two\nstatus: open\ntags: [x]\nderived_from: [Q001]\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
        "two",
    );
    finish_fixture(&temp);
    let error = Corpus::open(temp.path()).unwrap().snapshot().unwrap_err();
    assert!(error.to_string().contains("Lineage cycle at Q001"));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_outside_corpus() {
    use std::os::unix::fs::symlink;

    let temp = start_fixture();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside").unwrap();
    symlink(outside.path(), temp.path().join("questions/Q001-escape.md")).unwrap();
    finish_fixture(&temp);
    let error = Corpus::open(temp.path()).unwrap().snapshot().unwrap_err();
    assert!(error.to_string().contains("Corpus symlink refused"));
}

#[test]
fn rejects_invalid_frontmatter() {
    let temp = start_fixture();
    write(temp.path(), "questions/Q001-invalid.md", "plain markdown");
    finish_fixture(&temp);
    let error = Corpus::open(temp.path()).unwrap().snapshot().unwrap_err();
    assert!(error.to_string().contains("Missing frontmatter"));
}
