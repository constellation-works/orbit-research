use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn output(args: &[&str]) -> Output {
    output_with_format(Some("json"), args)
}

fn output_with_format(format: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(BINARY);
    if let Some(format) = format {
        command.args(["--format", format]);
    }
    command
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Research fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("run orbit-research")
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
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn check_script(script: &Path) -> Output {
    let binary_dir = PathBuf::from(BINARY)
        .parent()
        .expect("binary has a parent directory")
        .to_path_buf();
    let mut path = binary_dir.into_os_string();
    path.push(":");
    path.push(env::var_os("PATH").unwrap_or_default());
    Command::new("sh")
        .arg(script)
        .env("PATH", path)
        .output()
        .expect("run generated check script")
}

#[test]
fn local_research_works_without_backend_configuration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let init = output(&[
        "workspace",
        "init",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let listed = output(&[
        "research",
        "list",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let value: Value = serde_json::from_slice(&listed.stdout).expect("list JSON");
    assert!(value.is_object());
}

#[test]
fn check_cli_and_generated_script_report_a_summary_without_changing_the_corpus() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let initialized = output(&[
        "workspace",
        "init",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(initialized.status.success());

    let check_args = [
        "research",
        "check",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ];
    let structured = output_with_format(Some("json"), &check_args);
    assert!(structured.status.success());
    let summary: Value = serde_json::from_slice(&structured.stdout).expect("check JSON");
    assert_eq!(summary["valid"], true);
    assert_eq!(
        summary["base_revision"],
        git(&corpus, &["rev-parse", "HEAD"])
    );
    assert_eq!(summary["record_count"], 0);
    assert_eq!(summary["tag_count"], 0);
    assert!(summary.get("records").is_none());
    assert!(summary.get("tags").is_none());

    let human = output_with_format(None, &check_args);
    assert!(human.status.success());
    let human_text = String::from_utf8(human.stdout).expect("human output is UTF-8");
    assert_eq!(
        human_text,
        format!(
            "Corpus validation passed at base revision {}: 0 records, 0 tags.\n",
            summary["base_revision"].as_str().expect("revision string")
        )
    );

    let ndjson = output_with_format(Some("ndjson"), &check_args);
    assert!(ndjson.status.success());
    let ndjson = String::from_utf8(ndjson.stdout).expect("NDJSON output is UTF-8");
    assert_eq!(ndjson.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(ndjson.trim()).expect("check NDJSON JSON"),
        summary
    );

    let listed = output(&[
        "research",
        "list",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(listed.status.success());
    let snapshot: Value = serde_json::from_slice(&listed.stdout).expect("list JSON");
    assert!(snapshot.get("revision").is_some());
    assert!(snapshot["records"].is_array());
    assert!(snapshot["tags"].is_array());
    assert!(snapshot.get("valid").is_none());

    let head_before = git(&corpus, &["rev-parse", "HEAD"]);
    let status_before = git(&corpus, &["status", "--porcelain"]);
    let script = corpus.join("_scripts/check.sh");
    let script_text = fs::read_to_string(&script).expect("generated check script");
    assert!(script_text.contains("base revision and record/tag counts"));
    let readme = fs::read_to_string(corpus.join("README.md")).expect("generated guidance");
    assert!(readme.contains("record/tag counts"));
    let scripted = check_script(&script);
    assert!(
        scripted.status.success(),
        "{}",
        String::from_utf8_lossy(&scripted.stderr)
    );
    assert_eq!(
        String::from_utf8(scripted.stdout).expect("script output is UTF-8"),
        human_text
    );
    assert_eq!(git(&corpus, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&corpus, &["status", "--porcelain"]), status_before);
}

#[test]
fn invalid_corpus_check_fails_on_stderr_in_human_and_machine_modes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let initialized = output(&[
        "workspace",
        "init",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(initialized.status.success());
    fs::write(
        corpus.join("questions/Q001-invalid.md"),
        "not canonical frontmatter\n",
    )
    .expect("write invalid corpus fixture");

    let check_args = [
        "research",
        "check",
        "--corpus",
        corpus.to_str().expect("UTF-8 fixture path"),
    ];
    let head_before = git(&corpus, &["rev-parse", "HEAD"]);
    let status_before = git(&corpus, &["status", "--porcelain"]);

    let human = output_with_format(None, &check_args);
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    assert!(String::from_utf8_lossy(&human.stderr).contains("Missing frontmatter"));

    for format in ["json", "ndjson"] {
        let failed = output_with_format(Some(format), &check_args);
        assert_eq!(failed.status.code(), Some(1), "format: {format}");
        assert!(failed.stdout.is_empty(), "format: {format}");
        let error: Value = serde_json::from_slice(&failed.stderr).expect("structured error");
        assert_eq!(error["error"]["code"], "invalid-input");
        assert!(
            error["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("Missing frontmatter")
        );
    }

    let scripted = check_script(&corpus.join("_scripts/check.sh"));
    assert_eq!(scripted.status.code(), Some(1));
    assert!(scripted.stdout.is_empty());
    assert!(String::from_utf8_lossy(&scripted.stderr).contains("Missing frontmatter"));
    assert_eq!(git(&corpus, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&corpus, &["status", "--porcelain"]), status_before);
}

#[test]
fn mcp_protocol_responses_are_json_lines_on_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let init = output(&[
        "workspace",
        "init",
        corpus.to_str().expect("UTF-8 fixture path"),
    ]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let mut child = Command::new(BINARY)
        .args([
            "mcp",
            "--corpus",
            corpus.to_str().expect("UTF-8 fixture path"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn MCP");
    {
        let stdin = child.stdin.as_mut().expect("MCP stdin");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
        )
        .expect("initialize request");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
        )
        .expect("tools request");
    }
    let result = child.wait_with_output().expect("MCP output");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "MCP diagnostics leaked: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lines = String::from_utf8(result.stdout).expect("MCP UTF-8");
    assert!(lines.lines().count() >= 2, "MCP responses: {lines:?}");
    for line in lines.lines() {
        let response: Value = serde_json::from_str(line).expect("MCP JSON line");
        assert_eq!(response["jsonrpc"], "2.0");
    }
}
