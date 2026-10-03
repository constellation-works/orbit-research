//! What `revise` and `revise-question` do to the fields a caller does not name,
//! through the real binary and the MCP-shaped operation input.
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn run(args: &[&str]) -> Output {
    Command::new(BINARY)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("ORBIT_BIN", "/nonexistent/orbit")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("run orbit-research")
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

fn json_ok(args: &[&str]) -> Value {
    let output = run(&[&["--json"][..], args].concat());
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn stderr_of(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn show(root: &str, id: &str) -> Value {
    json_ok(&["research", "show", "--corpus", root, "--id", id])
}

fn blob(shown: &Value) -> String {
    shown["git_blob"].as_str().expect("blob").to_owned()
}

fn capture(root: &str) -> Value {
    json_ok(&[
        "research",
        "capture",
        "--corpus",
        root,
        "--text",
        "Why is the cache slow?\nIt got slower after the March deploy.",
        "--tag",
        "perf",
        "--tag",
        "cache",
    ]);
    show(root, "Q001")
}

fn hypothesis(root: &str) -> Value {
    json_ok(&[
        "research",
        "create",
        "--corpus",
        root,
        "--kind",
        "H",
        "--title",
        "Eviction thrash",
        "--body",
        "# H001\n\nThe cache evicts hot keys.",
        "--tag",
        "perf",
        "--request-key",
        "h1",
    ]);
    show(root, "H001")
}

#[test]
fn revise_question_with_only_a_title_keeps_the_body_and_tags() {
    let temp = corpus();
    let root = path(temp.path());
    let before = capture(root);
    let revised = json_ok(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&before),
        "--title",
        "Why did the cache get slower?",
    ]);
    assert_eq!(revised["mode"], "primary");
    assert!(revised.get("changed").is_none(), "{revised}");
    let after = show(root, "Q001");
    assert_eq!(after["metadata"]["title"], "Why did the cache get slower?");
    assert_eq!(after["metadata"]["tags"], json!(["perf", "cache"]));
    let body = after["body"].as_str().expect("body");
    assert!(
        body.contains("It got slower after the March deploy."),
        "{body}"
    );
    assert!(
        body.contains("# Q001 — Why did the cache get slower?"),
        "{body}"
    );
}

#[test]
fn revise_question_changes_exactly_the_fields_it_is_given() {
    let temp = corpus();
    let root = path(temp.path());
    let first = capture(root);
    // Tags alone leave the title and body alone.
    let tagged = json_ok(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&first),
        "--tag",
        "only",
    ]);
    assert!(tagged["commit"].is_string());
    let second = show(root, "Q001");
    assert_eq!(second["metadata"]["tags"], json!(["only"]));
    assert_eq!(second["metadata"]["title"], first["metadata"]["title"]);
    assert_eq!(second["body"], first["body"]);
    // A body alone leaves the title and tags alone.
    json_ok(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&second),
        "--body",
        "A new body.",
    ]);
    let third = show(root, "Q001");
    assert_eq!(third["metadata"]["tags"], json!(["only"]));
    assert_eq!(third["metadata"]["title"], first["metadata"]["title"]);
    assert!(
        third["body"]
            .as_str()
            .expect("body")
            .contains("A new body.")
    );
    // Clearing tags is explicit.
    json_ok(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&third),
        "--clear-tags",
    ]);
    assert_eq!(show(root, "Q001")["metadata"]["tags"], json!([]));
}

#[test]
fn revise_question_with_nothing_to_change_is_refused_and_names_the_fields() {
    let temp = corpus();
    let root = path(temp.path());
    let before = capture(root);
    let text = stderr_of(&[
        "research",
        "revise-question",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&before),
    ]);
    assert!(
        text.contains("Nothing to change: give at least one of title, body or tags"),
        "{text}"
    );
    let help = String::from_utf8(run(&["research", "revise-question", "--help"]).stdout)
        .expect("help text");
    assert!(help.contains("--clear-tags"), "{help}");
    assert!(help.contains("omitted fields keep"), "{help}");
}

#[test]
fn the_operation_input_keeps_omitted_fields_and_clears_tags_with_an_empty_list() {
    let temp = corpus();
    let root = path(temp.path());
    let before = capture(root);
    // This test commits in-process, so the identity must live in the repository.
    for (key, value) in [
        ("user.name", "Fixture"),
        ("user.email", "fixture@example.invalid"),
    ] {
        let set = Command::new("git")
            .args(["-C", root, "config", key, value])
            .status()
            .expect("git config");
        assert!(set.success());
    }
    let application = orbit_research_core::api::Application::local(temp.path()).expect("corpus");
    application
        .call(
            "research.revise_question",
            json!({"id": "Q001", "expected_blob": blob(&before), "title": "Renamed"}),
        )
        .expect("title-only revision");
    let renamed = show(root, "Q001");
    assert_eq!(renamed["metadata"]["tags"], json!(["perf", "cache"]));
    assert!(
        renamed["body"]
            .as_str()
            .expect("body")
            .contains("It got slower after the March deploy.")
    );
    let error = application
        .call(
            "research.revise_question",
            json!({"id": "Q001", "expected_blob": blob(&renamed)}),
        )
        .expect_err("no field to change");
    assert!(
        error.to_string().starts_with("Nothing to change"),
        "{error}"
    );
    application
        .call(
            "research.revise_question",
            json!({"id": "Q001", "expected_blob": blob(&renamed), "tags": []}),
        )
        .expect("empty list clears the tags");
    assert_eq!(show(root, "Q001")["metadata"]["tags"], json!([]));
}

#[test]
fn revise_clears_tags_only_when_asked_and_refuses_an_empty_hypothesis_body() {
    let temp = corpus();
    let root = path(temp.path());
    let before = hypothesis(root);
    let text = stderr_of(&[
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "H001",
        "--expected-blob",
        &blob(&before),
        "--body",
        "",
    ]);
    assert!(text.contains("A hypothesis needs a body"), "{text}");
    assert_eq!(show(root, "H001")["metadata"]["revision"], 1);

    json_ok(&[
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "H001",
        "--expected-blob",
        &blob(&before),
        "--clear-tags",
    ]);
    let cleared = show(root, "H001");
    assert_eq!(cleared["metadata"]["tags"], json!([]));
    assert_eq!(cleared["metadata"]["revision"], 1);
    assert_eq!(cleared["body"], before["body"]);

    let conflict = run(&[
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "H001",
        "--expected-blob",
        &blob(&cleared),
        "--clear-tags",
        "--tag",
        "x",
    ]);
    assert_eq!(conflict.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&conflict.stderr).contains("cannot be used with"),
        "{conflict:?}"
    );
}

#[test]
fn a_revise_that_changes_nothing_reports_changed_false() {
    let temp = corpus();
    let root = path(temp.path());
    let before = capture(root);
    let args = [
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&before),
        "--title",
        "Why is the cache slow?",
    ];
    let head = |root: &str| {
        let output = Command::new("git")
            .args(["-C", root, "rev-parse", "HEAD"])
            .output()
            .expect("git");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let commit_before = head(root);
    let receipt = json_ok(&args);
    assert_eq!(receipt["changed"], false, "{receipt}");
    assert_eq!(receipt["commit"], commit_before.as_str());
    assert_eq!(head(root), commit_before, "no commit was made");
    let human = String::from_utf8(run(&args).stdout).expect("UTF-8");
    assert!(
        human.starts_with("No changes: Q001 already has these values.\n"),
        "{human}"
    );
    assert!(!human.contains("changed:"), "{human}");

    let real = json_ok(&[
        "research",
        "revise",
        "--corpus",
        root,
        "--id",
        "Q001",
        "--expected-blob",
        &blob(&before),
        "--title",
        "Why is the cache so slow?",
    ]);
    assert!(real.get("changed").is_none(), "{real}");
    assert_ne!(head(root), commit_before);
}
