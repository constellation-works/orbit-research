use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::{Command, Output},
    sync::{Arc, Barrier},
    thread,
};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command(args: &[&str]) -> Command {
    let mut command = Command::new(BINARY);
    command
        .args(args)
        .env_remove("ORBIT_RESEARCH_FORMAT")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Research fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn run(args: &[&str]) -> Output {
    command(args).output().expect("run orbit-research")
}

fn json(args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    let output = run(&all);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn git(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn corpus(temp: &tempfile::TempDir) -> String {
    let corpus = temp.path().join("corpus");
    let corpus = corpus.to_str().expect("UTF-8 fixture path").to_owned();
    json(&["workspace", "init", &corpus]);
    corpus
}

#[test]
fn concurrent_primary_creates_in_separate_processes_get_distinct_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = corpus(&temp);
    const WRITERS: usize = 4;
    let start = Arc::new(Barrier::new(WRITERS));
    let writers: Vec<_> = (0..WRITERS)
        .map(|index| {
            let start = Arc::clone(&start);
            let corpus = corpus.clone();
            thread::spawn(move || {
                let key = format!("concurrent-{index}");
                let title = format!("Study {index}");
                let mut child = command(&[
                    "--json",
                    "research",
                    "create",
                    "--corpus",
                    &corpus,
                    "--kind",
                    "R",
                    "--status",
                    "planned",
                    "--title",
                    &title,
                    "--request-key",
                    &key,
                ]);
                // Release every process at once so their writes genuinely overlap.
                start.wait();
                child.output().expect("create process")
            })
        })
        .collect();
    let mut ids = BTreeSet::new();
    for writer in writers {
        let output = writer.join().expect("writer thread");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let created: Value = serde_json::from_slice(&output.stdout).expect("create JSON");
        assert_eq!(created["mode"], "primary");
        ids.insert(created["id"].as_str().expect("id").to_owned());
    }
    assert_eq!(
        ids,
        ["R001", "R002", "R003", "R004"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let root = Path::new(&corpus);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]), "5");
    assert!(git(root, &["status", "--porcelain"]).is_empty());
}

#[test]
fn captured_question_is_listed_by_a_fresh_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = corpus(&temp);
    let captured = json(&[
        "research",
        "capture",
        "--corpus",
        &corpus,
        "--text",
        "Why do the controls drift overnight?",
        "--tag",
        "inbox",
    ]);
    assert_eq!(captured["mode"], "primary");
    assert_eq!(captured["id"], "Q001");

    let listed = json(&["research", "list", "--corpus", &corpus]);
    let records = listed["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "Q001");
    assert_eq!(
        records[0]["metadata"]["title"],
        "Why do the controls drift overnight?"
    );
    assert_eq!(listed["tags"], serde_json::json!(["inbox"]));
}

#[test]
fn run_worktree_writes_only_its_reserved_research_without_committing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = corpus(&temp);
    json(&[
        "research",
        "create",
        "--corpus",
        &corpus,
        "--kind",
        "R",
        "--status",
        "planned",
        "--title",
        "Drift",
        "--request-key",
        "reserve-drift",
    ]);
    let linked = temp.path().join("run");
    let linked_path = linked.to_str().expect("UTF-8 path");
    git(
        Path::new(&corpus),
        &["worktree", "add", "-q", "--detach", linked_path],
    );
    let head = git(&linked, &["rev-parse", "HEAD"]);

    let body = temp.path().join("body.md");
    fs::write(
        &body,
        "## Question\n\nDrift?\n\n## Method\n\nOvernight run.\n\n## Result\n\nControls failed.\n\n## Limitations\n\nOne night.\n\n## Next\n\nRepeat.\n",
    )
    .expect("body");
    let manifest = temp.path().join("manifest.json");
    fs::write(&manifest, r#"{"inputs":[{"name":"night.csv"}]}"#).expect("manifest");
    let blob = json(&["research", "show", "--corpus", linked_path, "--id", "R001"])["git_blob"]
        .as_str()
        .expect("blob")
        .to_owned();
    let written = json(&[
        "research",
        "revise",
        "--corpus",
        linked_path,
        "--mode",
        "worktree",
        "--id",
        "R001",
        "--expected-blob",
        &blob,
        "--status",
        "done",
        "--orbit-task",
        "task-1",
        "--orbit-run",
        "run-1",
        "--body-file",
        body.to_str().expect("UTF-8 path"),
        "--manifest-file",
        manifest.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(written["mode"], "worktree");
    assert_eq!(written["id"], "R001");
    assert!(written.get("commit").is_none());
    assert_eq!(git(&linked, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&linked, &["status", "--porcelain"]),
        "M research/R001-drift/README.md\n M research/R001-drift/data/manifest.json"
    );

    // Allocation and primary-mode expectations refuse from the run worktree.
    for args in [
        &[
            "research",
            "create",
            "--corpus",
            linked_path,
            "--kind",
            "R",
            "--title",
            "Extra",
            "--request-key",
            "extra",
        ][..],
        &[
            "research",
            "capture",
            "--corpus",
            linked_path,
            "--text",
            "Another question?",
        ][..],
        &[
            "research",
            "revise",
            "--corpus",
            &corpus,
            "--mode",
            "worktree",
            "--id",
            "R001",
            "--expected-blob",
            &blob,
            "--status",
            "running",
        ][..],
    ] {
        let output = run(&[&["--json"][..], args].concat());
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let error: Value = serde_json::from_slice(&output.stderr).expect("structured error");
        let message = error["error"]["message"].as_str().expect("message");
        assert!(message.contains("mode"), "{args:?}: {message}");
    }
    assert_eq!(git(&linked, &["rev-parse", "HEAD"]), head);
    assert!(git(Path::new(&corpus), &["status", "--porcelain"]).is_empty());
}

#[test]
fn revise_question_and_work_links_are_cli_subcommands() {
    for command in [
        "revise-question",
        "work-links",
        "capture",
        "revise",
        "assess",
    ] {
        let output = run(&["research", command, "--help"]);
        assert!(output.status.success(), "{command} --help");
        let help = String::from_utf8(output.stdout).expect("help UTF-8");
        assert!(help.contains("--json"), "{command}: {help}");
        assert!(help.contains("--corpus"), "{command}: {help}");
    }
    let help = String::from_utf8(run(&["research", "--help"]).stdout).expect("help UTF-8");
    assert!(help.contains("Worktree mode"), "{help}");

    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = corpus(&temp);
    assert_eq!(
        json(&["research", "work-links", "--corpus", &corpus]),
        serde_json::json!([])
    );
    json(&[
        "research",
        "capture",
        "--corpus",
        &corpus,
        "--text",
        "Original question?",
    ]);
    let blob = json(&["research", "show", "--corpus", &corpus, "--id", "Q001"])["git_blob"]
        .as_str()
        .expect("blob")
        .to_owned();
    let revised = json(&[
        "research",
        "revise-question",
        "--corpus",
        &corpus,
        "--id",
        "Q001",
        "--expected-blob",
        &blob,
        "--title",
        "Sharper question?",
        "--body",
        "Sharper question?",
        "--tag",
        "inbox",
    ]);
    assert_eq!(revised["mode"], "primary");
    assert_eq!(revised["path"], "questions/Q001-original-question.md");
}
