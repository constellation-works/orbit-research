//! The messages a person or an agent reads when a corpus, a write or the
//! shared request storage is not in the state a command needs. Each test runs
//! the real binary and pins what the text says to do next.
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        // No test here may reach an Orbit installation.
        .env("ORBIT_BIN", "/nonexistent/orbit")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn run(args: &[&str]) -> Output {
    command().args(args).output().expect("run orbit-research")
}

fn path(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}

fn corpus() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let output = run(&["workspace", "init", path(temp.path())]);
    assert!(output.status.success(), "{output:?}");
    temp
}

/// Stderr of a command that must fail with exit 1.
fn failure(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("git");
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// One plugin tool call through the real `orbit-tool` transport.
fn plugin(tool: &str, workspace: &Path, input: Value) -> Value {
    let request = json!({"tool": tool, "input": input, "context": {"workspace_root": workspace}});
    let mut child = command()
        .arg("orbit-tool")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn orbit-tool");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.to_string().as_bytes())
        .expect("write request");
    let output = child.wait_with_output().expect("reply");
    serde_json::from_slice(&output.stdout).expect("one JSON reply")
}

fn error_of(reply: &Value) -> (&str, &str) {
    assert_eq!(reply["ok"], false, "{reply}");
    (
        reply["error"]["code"].as_str().expect("code"),
        reply["error"]["message"].as_str().expect("message"),
    )
}

fn break_corpus(root: &Path) {
    std::fs::write(
        root.join("questions/Q001-bad.md"),
        "---\nid: Q001\ntitle: Bad\nslug: bad\nstatus: bogus\ntags: [x]\nderived_from: [Z9]\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []\n---\nbody\n",
    )
    .expect("bad record");
    std::fs::write(root.join("questions/Q002-plain.md"), "just text\n").expect("plain file");
}

#[test]
fn check_list_and_show_report_every_problem_in_plain_words() {
    let temp = corpus();
    break_corpus(temp.path());
    for verb in [["check"].as_slice(), &["list"], &["show", "--id", "Q001"]] {
        let mut args = vec!["research"];
        args.push(verb[0]);
        args.extend(["--corpus", path(temp.path())]);
        args.extend(&verb[1..]);
        let text = failure(&args);
        assert!(
            text.contains("3 problems in the corpus:"),
            "{verb:?}: {text}"
        );
        assert!(
            text.contains("questions/Q001-bad.md: status: \"bogus\" is not allowed; allowed values: open, answered, dropped"),
            "{text}"
        );
        assert!(
            text.contains("questions/Q001-bad.md: derived_from[0]: \"Z9\" is not a record id; expected an id like Q001"),
            "{text}"
        );
        assert!(
            text.contains("questions/Q002-plain.md: missing frontmatter"),
            "{text}"
        );
        for raw in ["oneOf", "is not valid under any", "^[", "{\""] {
            assert!(!text.contains(raw), "{raw}: {text}");
        }
    }
}

#[test]
fn json_output_keeps_the_problems_structured() {
    let temp = corpus();
    break_corpus(temp.path());
    let stderr = failure(&["--json", "research", "check", "--corpus", path(temp.path())]);
    let error: Value = serde_json::from_str(&stderr).expect("structured error");
    let problems = error["error"]["problems"]
        .as_array()
        .expect("problems array");
    assert_eq!(problems.len(), 3, "{error}");
    for problem in problems {
        assert!(
            problem["path"].is_string() && problem["message"].is_string(),
            "{problem}"
        );
        assert!(problem.get("field").is_some(), "{problem}");
    }
    assert_eq!(problems[0]["path"], "questions/Q001-bad.md");
    assert_eq!(problems[0]["field"], "derived_from[0]");
}

#[test]
fn the_plugin_reports_the_same_problems_with_a_corpus_code() {
    let temp = corpus();
    break_corpus(temp.path());
    for tool in ["check", "list"] {
        let reply = plugin(tool, temp.path(), json!({}));
        let (code, message) = error_of(&reply);
        assert_eq!(code, "corpus_unavailable", "{tool}");
        assert!(
            message.starts_with("3 problems in the corpus:"),
            "{message}"
        );
        assert_eq!(
            reply["error"]["problems"]
                .as_array()
                .expect("problems")
                .len(),
            3
        );
        assert!(!message.contains("oneOf"), "{message}");
    }
}

#[test]
fn a_yaml_error_names_its_file_through_the_plugin_too() {
    let temp = corpus();
    std::fs::write(
        temp.path().join("questions/Q001-x.md"),
        "---\nid: [open\n---\nb\n",
    )
    .expect("fixture step");
    let reply = plugin("list", temp.path(), json!({}));
    let (code, message) = error_of(&reply);
    assert_eq!(code, "corpus_unavailable");
    assert!(
        message.starts_with("questions/Q001-x.md: frontmatter is not valid YAML:"),
        "{message}"
    );
}

#[test]
fn empty_text_and_bad_ids_say_what_is_wrong_without_regexes() {
    let temp = corpus();
    let root = path(temp.path());
    let text = failure(&["research", "capture", "--corpus", root, "--text", ""]);
    assert!(
        text.contains("`text` is required and must not be empty"),
        "{text}"
    );
    assert!(!text.contains("shorter than"), "{text}");
    for id in ["X1", "q1"] {
        let text = failure(&["research", "show", "--corpus", root, "--id", id]);
        assert!(text.contains("expected an id like Q001"), "{text}");
        assert!(!text.contains("^["), "{text}");
    }
    let text = failure(&[
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "Q1",
        "--expected-blob",
        "abc",
    ]);
    assert!(text.contains("expected an id like Q001"), "{text}");
    let reply = plugin("show", temp.path(), json!({"id": "Q1"}));
    let (code, message) = error_of(&reply);
    assert_eq!(
        (code, message.contains("expected an id like Q001")),
        ("invalid_request", true)
    );
}

#[test]
fn a_dirty_checkout_lists_the_offending_paths() {
    let temp = corpus();
    std::fs::write(temp.path().join("stray.txt"), "x").expect("fixture step");
    let text = failure(&[
        "research",
        "capture",
        "--corpus",
        path(temp.path()),
        "--text",
        "A question",
    ]);
    assert!(text.contains("clean corpus integration checkout"), "{text}");
    assert!(text.contains("\n  ?? stray.txt"), "{text}");
}

#[test]
fn prepare_operations_makes_git_ignore_scratch_and_init_already_does() {
    let temp = corpus();
    // `workspace init` writes the ignore rule into the tracked .gitignore.
    assert!(
        std::fs::read_to_string(temp.path().join(".gitignore"))
            .expect("fixture step")
            .contains("/.orbit-research-tmp/\n")
    );
    std::fs::create_dir(temp.path().join(".orbit-research-tmp")).expect("fixture step");
    std::fs::write(temp.path().join(".orbit-research-tmp/a.json"), "{}").expect("fixture step");
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), "");
    let output = run(&[
        "research",
        "capture",
        "--corpus",
        path(temp.path()),
        "--text",
        "Fine now",
    ]);
    assert!(output.status.success(), "{output:?}");

    // An older corpus without the rule gets it from prepare-operations, locally.
    let old = corpus();
    std::fs::write(
        old.path().join(".gitignore"),
        "_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )
    .expect("fixture step");
    git(old.path(), &["commit", "-qam", "older ignore rules"]);
    let first = run(&[
        "--json",
        "workspace",
        "prepare-operations",
        path(old.path()),
    ]);
    assert!(first.status.success(), "{first:?}");
    let receipt: Value = serde_json::from_slice(&first.stdout).expect("fixture step");
    assert_eq!(receipt["scratch_ignored"], true);
    std::fs::create_dir(old.path().join(".orbit-research-tmp")).expect("fixture step");
    std::fs::write(old.path().join(".orbit-research-tmp/a.json"), "{}").expect("fixture step");
    assert_eq!(git(old.path(), &["status", "--porcelain"]), "");
    let again = run(&[
        "--json",
        "workspace",
        "prepare-operations",
        path(old.path()),
    ]);
    let receipt: Value = serde_json::from_slice(&again.stdout).expect("fixture step");
    assert_eq!(
        (
            receipt["changed"].clone(),
            receipt.get("scratch_ignored").is_none()
        ),
        (json!(false), true)
    );
}

#[test]
fn a_missing_operations_directory_is_named_and_prepare_operations_restores_it() {
    let temp = corpus();
    let root = path(temp.path());
    let state = temp.path().join("_data/orbit-research-operations");
    std::fs::remove_dir_all(&state).expect("fixture step");

    let text = failure(&["research", "work-links", "--corpus", root]);
    assert!(!text.contains("os error"), "{text}");
    assert!(
        text.contains(state.canonicalize_parent().as_str()),
        "{text}"
    );
    assert!(text.contains("workspace prepare-operations"), "{text}");

    let reply = plugin(
        "link",
        temp.path(),
        json!({"research_id": "R001", "request_key": "k", "title": "t"}),
    );
    let (code, message) = error_of(&reply);
    assert_eq!(code, "refused");
    assert!(
        message.contains("_data/orbit-research-operations") && !message.contains("os error"),
        "{message}"
    );

    let output = run(&["--json", "workspace", "prepare-operations", root]);
    assert!(output.status.success(), "{output:?}");
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("fixture step");
    assert_eq!(receipt["recreated"], true);
    let output = run(&["--json", "research", "work-links", "--corpus", root]);
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "[]");
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), "");
}

trait CanonicalParent {
    fn canonicalize_parent(&self) -> String;
}
impl CanonicalParent for std::path::PathBuf {
    /// The state path as the binary prints it: the corpus root is canonical,
    /// and the state directory itself no longer exists to canonicalize.
    fn canonicalize_parent(&self) -> String {
        let parent = self
            .parent()
            .expect("parent")
            .canonicalize()
            .expect("canonical parent");
        parent
            .join(self.file_name().expect("name"))
            .display()
            .to_string()
    }
}

#[test]
fn plan_names_the_fix_for_an_unreserved_or_wrong_kind_research_id() {
    let temp = corpus();
    let root = path(temp.path());
    let output = run(&[
        "research",
        "capture",
        "--corpus",
        root,
        "--text",
        "A question",
    ]);
    assert!(output.status.success(), "{output:?}");
    for (id, code) in [("R009", "record_not_found"), ("Q001", "invalid_request")] {
        let reply = plugin(
            "plan",
            temp.path(),
            json!({"shape": "investigation", "research_id": id, "objective": "x"}),
        );
        let (got, message) = error_of(&reply);
        assert_eq!(got, code, "{id}: {message}");
        assert!(
            message.contains("research create --kind R --status planned"),
            "{id}: {message}"
        );
        assert!(message.contains(id), "{message}");
    }
    let text = failure(&[
        "research",
        "plan",
        "--corpus",
        root,
        "--shape",
        "investigation",
        "--research-id",
        "R009",
        "--objective",
        "x",
    ]);
    assert!(
        text.contains("research create --kind R --status planned"),
        "{text}"
    );
}

#[test]
fn an_unusable_request_key_is_an_invalid_request_with_a_plain_range() {
    let temp = corpus();
    for key in [String::new(), "k".repeat(300)] {
        let reply = plugin(
            "link",
            temp.path(),
            json!({"research_id": "R001", "request_key": key, "title": "t"}),
        );
        let (code, message) = error_of(&reply);
        assert_eq!(code, "invalid_request");
        assert!(message.contains("1-256 bytes"), "{message}");
    }
    let text = failure(&[
        "research",
        "create",
        "--corpus",
        path(temp.path()),
        "--kind",
        "Q",
        "--title",
        "x",
        "--request-key",
        "",
    ]);
    assert!(
        text.contains("`request_key` is required and must not be empty"),
        "{text}"
    );
}

#[test]
fn revise_question_on_a_hypothesis_points_to_revise() {
    let temp = corpus();
    let root = path(temp.path());
    let created = run(&[
        "--json",
        "research",
        "create",
        "--corpus",
        root,
        "--kind",
        "H",
        "--title",
        "Hyp",
        "--request-key",
        "h1",
    ]);
    assert!(created.status.success(), "{created:?}");
    let shown = run(&[
        "--json", "research", "show", "--corpus", root, "--id", "H001",
    ]);
    let blob = serde_json::from_slice::<Value>(&shown.stdout).expect("fixture step")["git_blob"]
        .as_str()
        .expect("fixture step")
        .to_owned();
    let text = failure(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "H001",
        "--expected-blob",
        &blob,
        "--title",
        "Hyp",
    ]);
    assert!(
        text.contains("revise-question edits questions only"),
        "{text}"
    );
    assert!(text.contains("`research revise --id H001`"), "{text}");
    assert!(!text.contains("Only existing questions"), "{text}");
}

#[test]
fn stale_blob_and_reused_key_messages_say_what_to_do() {
    let temp = corpus();
    let root = path(temp.path());
    let ok = run(&["research", "capture", "--corpus", root, "--text", "First"]);
    assert!(ok.status.success(), "{ok:?}");
    let text = failure(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        "deadbeef",
        "--title",
        "Other",
    ]);
    assert!(
        text.contains("run `research show --id Q001` again and use its `git_blob`"),
        "{text}"
    );
    let key = [
        "research",
        "create",
        "--corpus",
        root,
        "--kind",
        "Q",
        "--request-key",
        "same",
        "--title",
    ];
    let first = run(&[&key[..], &["Reuse a"]].concat());
    assert!(first.status.success(), "{first:?}");
    let text = failure(&[&key[..], &["Reuse b"]].concat());
    assert!(
        text.contains("omit the request key") && text.contains("choose a new key"),
        "{text}"
    );
}

#[test]
fn a_missing_corpus_suggests_workspace_init() {
    let temp = tempfile::tempdir().expect("fixture step");
    let missing = temp.path().join("nope");
    let text = failure(&["research", "check", "--corpus", path(&missing)]);
    assert!(text.contains("the path does not exist"), "{text}");
    assert!(
        text.contains(&format!(
            "`orbit-research workspace init {}`",
            missing.display()
        )),
        "{text}"
    );
    let empty = temp.path().join("empty");
    std::fs::create_dir(&empty).expect("fixture step");
    let text = failure(&["research", "check", "--corpus", path(&empty)]);
    assert!(text.contains("`orbit-research workspace init "), "{text}");
    let reply = plugin("check", &missing, json!({}));
    let (code, message) = error_of(&reply);
    assert_eq!(code, "corpus_unavailable");
    assert!(message.contains("workspace init"), "{message}");
}
