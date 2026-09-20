use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn output(args: &[&str]) -> std::process::Output {
    Command::new(BINARY)
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
