use orbit_research_core::{
    Research,
    receipt::{Artifact, Receipt},
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");
const TASK_ID: &str = "T-123";
const RUN_ID: &str = "run-123";
const WORKSPACE: &str = "ws-test";
const MACHINE: &str = "machine-test";
const RECORD_PATH: &str = "research/R001-study/README.md";
const ARTIFACT_PATH: &str = "research/R001-study/data/result.txt";

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

fn fixture(body: &str) -> (TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    fs::create_dir_all(root.join("research/R001-study/data")).unwrap();
    let front = format!(
        "id: R001\ntitle: Study\nstatus: done\ntags: [test]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: []\norbit:\n  task: {TASK_ID}\n  run: {RUN_ID}\n"
    );
    fs::write(root.join(RECORD_PATH), format!("---\n{front}---\n{body}")).unwrap();
    fs::write(root.join(ARTIFACT_PATH), b"artifact bytes\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
    git(root, &["config", "user.name", "Receipt tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "published result"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    git(root, &["update-ref", "refs/remotes/origin/main", &commit]);
    (temp, commit)
}

fn bytes_at(root: &Path, commit: &str, path: &str) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{commit}:{path}")])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

fn receipt(root: &Path, commit: &str) -> Receipt {
    let record_blob = git(root, &["rev-parse", &format!("{commit}:{RECORD_PATH}")]);
    let artifact_hash = format!(
        "{:x}",
        Sha256::digest(bytes_at(root, commit, ARTIFACT_PATH))
    );
    Receipt {
        schema_version: 1,
        workspace: WORKSPACE.into(),
        owner_machine_id: MACHINE.into(),
        task_id: TASK_ID.into(),
        run_id: RUN_ID.into(),
        record_id: "R001".into(),
        record_path: RECORD_PATH.into(),
        corpus_commit: commit.into(),
        record_blob,
        artifacts: vec![Artifact {
            path: ARTIFACT_PATH.into(),
            sha256: artifact_hash,
        }],
    }
}

fn context() -> (serde_json::Value, serde_json::Value) {
    (
        serde_json::json!({"id": TASK_ID, "job_run_id": RUN_ID, "status": "done"}),
        serde_json::json!({"run_id": RUN_ID, "state": "success", "executed_on": {"machine_id": MACHINE}}),
    )
}

fn clone_receipt(receipt: &Receipt) -> Receipt {
    serde_json::from_value(serde_json::to_value(receipt).unwrap()).unwrap()
}

const COMPLETE: &str = "## Question\nWhat was tested?\n\n## Method\nA controlled run.\n\n## Result\nThe result was recorded.\n\n## Limitations\nOne fixture only.\n\n## Next\nRepeat with more data.\n";

#[test]
fn accepts_valid_published_receipt_and_exact_artifact_evidence() {
    let (temp, commit) = fixture(COMPLETE);
    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let accepted = research
        .validate_receipt(
            &receipt(temp.path(), &commit),
            &task,
            &run,
            WORKSPACE,
            MACHINE,
            "refs/remotes/origin/main",
        )
        .unwrap();
    assert_eq!(accepted.record_id, "R001");
    assert_eq!(accepted.task_id, TASK_ID);
    assert_eq!(accepted.run_id, RUN_ID);
    assert_eq!(accepted.corpus_commit, commit);
}

#[test]
fn rejects_unpublished_commit() {
    let (temp, published) = fixture(COMPLETE);
    fs::write(temp.path().join(ARTIFACT_PATH), b"new unpublished bytes\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-q", "-m", "unpublished"]);
    let unpublished = git(temp.path(), &["rev-parse", "HEAD"]);
    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let error = research
        .validate_receipt(
            &receipt(temp.path(), &unpublished),
            &task,
            &run,
            WORKSPACE,
            MACHINE,
            "refs/remotes/origin/main",
        )
        .unwrap_err();
    assert!(error.to_string().contains("not reachable"));
    assert_ne!(published, unpublished);
}

#[test]
fn rejects_task_run_and_execution_host_mismatches() {
    let (temp, commit) = fixture(COMPLETE);
    let research = Research::open(temp.path()).unwrap();
    let good = receipt(temp.path(), &commit);
    let (task, run) = context();
    let mut wrong_task = clone_receipt(&good);
    wrong_task.task_id = "other-task".into();
    assert!(
        research
            .validate_receipt(
                &wrong_task,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
    let mut wrong_run = clone_receipt(&good);
    wrong_run.run_id = "other-run".into();
    assert!(
        research
            .validate_receipt(
                &wrong_run,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
    let bad_host = serde_json::json!({"run_id": RUN_ID, "state": "success", "executed_on": {"machine_id": "other-machine"}});
    assert!(
        research
            .validate_receipt(
                &good,
                &task,
                &bad_host,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
    assert!(
        research
            .validate_receipt(
                &good,
                &task,
                &run,
                "other-workspace",
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
}

#[test]
fn rejects_incorrect_record_blob_and_artifact_hash() {
    let (temp, commit) = fixture(COMPLETE);
    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let mut wrong_blob = receipt(temp.path(), &commit);
    wrong_blob.record_blob = "0".repeat(40);
    assert!(
        research
            .validate_receipt(
                &wrong_blob,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
    let mut wrong_artifact = receipt(temp.path(), &commit);
    wrong_artifact.artifacts[0].sha256 = "0".repeat(64);
    assert!(
        research
            .validate_receipt(
                &wrong_artifact,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
}

#[test]
fn rejects_cross_item_artifact_and_missing_or_scaffold_sections() {
    let (temp, commit) = fixture(COMPLETE);
    fs::create_dir_all(temp.path().join("research/R002-other/data")).unwrap();
    fs::write(
        temp.path().join("research/R002-other/data/result.txt"),
        b"other\n",
    )
    .unwrap();
    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let mut cross = receipt(temp.path(), &commit);
    cross.artifacts[0].path = "research/R002-other/data/result.txt".into();
    assert!(
        research
            .validate_receipt(
                &cross,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );

    let (scaffold_temp, scaffold_commit) = fixture(
        "## Question\nPending.\n\n## Method\nA method.\n\n## Result\nA result.\n\n## Limitations\nA limit.\n\n## Next\nA next step.\n",
    );
    let scaffold = Research::open(scaffold_temp.path()).unwrap();
    let scaffold_receipt = receipt(scaffold_temp.path(), &scaffold_commit);
    assert!(
        scaffold
            .validate_receipt(
                &scaffold_receipt,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );

    let (missing_temp, missing_commit) = fixture(
        "## Question\nA question.\n\n## Method\nA method.\n\n## Result\nA result.\n\n## Limitations\nA limit.\n",
    );
    let missing = Research::open(missing_temp.path()).unwrap();
    let missing_receipt = receipt(missing_temp.path(), &missing_commit);
    assert!(
        missing
            .validate_receipt(
                &missing_receipt,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
}

#[test]
fn rejects_duplicate_artifact_paths() {
    let (temp, commit) = fixture(COMPLETE);
    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let mut duplicate = receipt(temp.path(), &commit);
    let path = duplicate.artifacts[0].path.clone();
    let sha256 = duplicate.artifacts[0].sha256.clone();
    duplicate.artifacts.push(Artifact { path, sha256 });
    assert!(
        research
            .validate_receipt(
                &duplicate,
                &task,
                &run,
                WORKSPACE,
                MACHINE,
                "refs/remotes/origin/main"
            )
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_artifact_even_when_the_link_is_committed() {
    use std::os::unix::fs::symlink;

    let (temp, _) = fixture(COMPLETE);
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), b"outside bytes\n").unwrap();
    fs::remove_file(temp.path().join(ARTIFACT_PATH)).unwrap();
    symlink(outside.path(), temp.path().join(ARTIFACT_PATH)).unwrap();
    git(temp.path(), &["add", "-A"]);
    git(temp.path(), &["commit", "-q", "-m", "symlink artifact"]);
    let commit = git(temp.path(), &["rev-parse", "HEAD"]);
    git(
        temp.path(),
        &["update-ref", "refs/remotes/origin/main", &commit],
    );

    let research = Research::open(temp.path()).unwrap();
    let (task, run) = context();
    let error = research
        .validate_receipt(
            &receipt(temp.path(), &commit),
            &task,
            &run,
            WORKSPACE,
            MACHINE,
            "refs/remotes/origin/main",
        )
        .unwrap_err();
    assert!(error.to_string().contains("regular committed files"));
}
