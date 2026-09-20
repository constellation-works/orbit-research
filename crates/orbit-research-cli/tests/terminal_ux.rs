use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Research fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn run(args: &[&str]) -> Output {
    command().args(args).output().expect("run orbit-research")
}

fn run_with_env(args: &[&str], key: &str, value: &str) -> Output {
    command()
        .args(args)
        .env(key, value)
        .output()
        .expect("run orbit-research with environment")
}

fn initialize(corpus: &Path) {
    let output = run(&[
        "workspace",
        "init",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_success(&output);
}

fn create_question(corpus: &Path) {
    let output = run(&[
        "--format",
        "json",
        "research",
        "create",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
        "--kind",
        "Q",
        "--title",
        "Terminal output contract",
        "--body",
        "Full record body for terminal detail.",
        "--request-key",
        "terminal-ux-fixture",
    ]);
    assert_success(&output);
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!(
            "expected JSON ({error}), got: {}",
            String::from_utf8_lossy(bytes)
        )
    })
}

fn assert_structured_error(output: &Output, expected_exit: i32) {
    assert_exit(output, expected_exit);
    assert!(output.stdout.is_empty(), "errors must not leak to stdout");
    let error = parse_json(&output.stderr);
    assert!(error["error"]["code"].is_string(), "{error:#}");
    assert!(error["error"]["message"].is_string(), "{error:#}");
}

fn assert_plain_error(output: &Output, expected_exit: i32) {
    assert_exit(output, expected_exit);
    assert!(output.stdout.is_empty(), "errors must not leak to stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.trim().is_empty(),
        "plain error must explain the failure"
    );
    assert!(
        serde_json::from_slice::<Value>(&output.stderr).is_err(),
        "plain mode emitted a JSON error: {stderr}"
    );
    assert!(
        !stderr.contains("\u{1b}["),
        "plain error contained ANSI: {stderr:?}"
    );
}

#[test]
fn bare_invocation_prints_human_help_to_stdout() {
    let output = run(&[]);
    assert_success(&output);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("research"), "{stdout}");
    assert!(!stdout.trim_start().starts_with('{'), "{stdout}");
}

#[test]
fn help_flags_and_nested_command_help_are_documented_successes() {
    for args in [
        &["-h"][..],
        &["--help"][..],
        &["research", "--help"][..],
        &["research", "create", "--help"][..],
    ] {
        let output = run(args);
        assert_success(&output);
        assert!(
            output.stderr.is_empty(),
            "help wrote to stderr for {args:?}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Usage:"),
            "missing usage for {args:?}: {stdout}"
        );
        assert!(
            stdout.contains("--help"),
            "missing help flag for {args:?}: {stdout}"
        );
    }
}

#[test]
fn version_flags_print_the_package_version() {
    for flag in ["--version", "-V"] {
        let output = run(&[flag]);
        assert_success(&output);
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(env!("CARGO_PKG_VERSION")),
            "{flag}: {stdout}"
        );
    }
}

#[test]
fn resource_version_option_is_not_confused_with_cli_version_help() {
    let output = run(&["resource", "--version", "1"]);
    assert_success(&output);
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("research-native"), "{stdout}");
    assert!(stdout.contains("orbit-research research list"), "{stdout}");
}

#[test]
fn usage_errors_are_plain_by_default_and_exit_two() {
    for args in [&["not-a-command"][..], &["research", "create"][..]] {
        assert_plain_error(&run(args), 2);
    }
}

#[test]
fn explicit_json_usage_error_is_structured_and_exits_two() {
    assert_structured_error(&run(&["--format", "json", "not-a-command"]), 2);
}

#[test]
fn operational_errors_follow_the_selected_output_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing-corpus");
    let corpus = missing.to_str().expect("UTF-8 fixture path");

    assert_plain_error(&run(&["research", "list", "--corpus", corpus]), 1);
    assert_structured_error(
        &run(&["--format", "json", "research", "list", "--corpus", corpus]),
        1,
    );
}

#[test]
fn piped_list_is_headerless_plain_text_and_explicit_json_is_canonical() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    initialize(&corpus);
    create_question(&corpus);
    let corpus = corpus.to_str().expect("UTF-8 fixture path");

    let plain = run(&["research", "list", "--corpus", corpus]);
    assert_success(&plain);
    assert!(plain.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(stdout.contains("Q001"), "{stdout}");
    assert!(
        !stdout.starts_with("ID\t"),
        "pipe output has a header: {stdout}"
    );
    assert!(
        !stdout.contains("\u{1b}["),
        "pipe output contained ANSI: {stdout:?}"
    );

    let json = run(&["--format", "json", "research", "list", "--corpus", corpus]);
    assert_success(&json);
    assert!(json.stderr.is_empty());
    let value = parse_json(&json.stdout);
    assert!(value.is_object(), "canonical list response: {value:#}");
    assert!(
        value["records"].is_array(),
        "canonical list response: {value:#}"
    );
}

#[test]
fn research_show_returns_the_complete_record_in_json_and_human_modes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    initialize(&corpus);
    create_question(&corpus);
    let corpus = corpus.to_str().expect("UTF-8 fixture path");

    let json = run(&[
        "--format", "json", "research", "show", "--corpus", corpus, "--id", "Q001",
    ]);
    assert_success(&json);
    assert!(json.stderr.is_empty());
    let record = parse_json(&json.stdout);
    assert_eq!(record["id"], "Q001");
    assert_eq!(record["metadata"]["title"], "Terminal output contract");
    assert_eq!(record["path"], "questions/Q001-terminal-output-contract.md");
    let body = record["body"].as_str().expect("record body string");
    assert!(body.contains("# Q001 — Terminal output contract"), "{body}");
    assert!(
        body.contains("Full record body for terminal detail."),
        "{body}"
    );

    let human = run(&["research", "show", "--corpus", corpus, "--id", "Q001"]);
    assert_success(&human);
    assert!(human.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&human.stdout);
    for expected in [
        "Q001",
        "Terminal output contract",
        "questions/Q001-terminal-output-contract.md",
        "Full record body for terminal detail.",
    ] {
        assert!(stdout.contains(expected), "missing {expected:?}: {stdout}");
    }
}

#[test]
fn empty_human_list_has_no_data_and_writes_a_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    initialize(&corpus);

    let output = run(&[
        "research",
        "list",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_success(&output);
    assert!(output.stdout.is_empty(), "empty list emitted data");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.trim().is_empty(), "empty list needs a diagnostic");
    assert!(
        !stderr.contains("\u{1b}["),
        "diagnostic contained ANSI: {stderr:?}"
    );
}

#[test]
fn format_environment_is_optional_and_explicit_auto_takes_precedence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    initialize(&corpus);
    create_question(&corpus);
    let args = [
        "research",
        "list",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ];

    let from_environment = run_with_env(&args, "ORBIT_RESEARCH_FORMAT", "json");
    assert_success(&from_environment);
    assert!(parse_json(&from_environment.stdout).is_object());

    let explicit = run_with_env(
        &["--format", "auto", "research", "list", "--corpus", args[3]],
        "ORBIT_RESEARCH_FORMAT",
        "json",
    );
    assert_success(&explicit);
    assert!(
        serde_json::from_slice::<Value>(&explicit.stdout).is_err(),
        "explicit auto did not override environment: {}",
        String::from_utf8_lossy(&explicit.stdout)
    );
    assert!(String::from_utf8_lossy(&explicit.stdout).contains("Q001"));
}
