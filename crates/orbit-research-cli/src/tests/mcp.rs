use super::super::mcp::serve_mcp_application;
use orbit_research_core::Application;
use serde_json::Value;
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../orbit-research-core/tests/fixtures/schema.json");

fn corpus() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).expect("schema directory");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("fixture schema");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("record directory");
    }
    let run = |args: &[&str]| {
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
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "mcp-tests@example.invalid"]);
    run(&["config", "user.name", "MCP tests"]);
    run(&["add", "."]);
    run(&["commit", "-q", "--allow-empty", "-m", "fixture"]);
    temp
}

fn exchange(root: &Path, input: &str) -> Vec<Value> {
    let mut output = Vec::new();
    serve_mcp_application(
        &Application::local(root).expect("local application"),
        Cursor::new(input.as_bytes()),
        &mut output,
    )
    .expect("serve fixture MCP requests");
    String::from_utf8(output)
        .expect("UTF-8 response")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

#[test]
fn handshake_discovery_and_stdout_are_protocol_pure() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":"l","method":"tools/list"}
"#,
    );
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["result"]["protocolVersion"], "2024-11-05");
    assert!(
        values[1]["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .any(|tool| tool["name"] == "research.create")
    );
    assert!(values.iter().all(|value| value["jsonrpc"] == "2.0"));
}

#[test]
fn invalid_framing_and_handshake_errors_are_bounded() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\nnot-json\n",
    );
    assert_eq!(values[0]["error"]["code"], -32002);
    assert_eq!(values[1]["error"]["code"], -32700);
    let oversized = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"x\":\"{}\"}}\n",
        "x".repeat(128 * 1024)
    );
    let mut out = Vec::new();
    let error = serve_mcp_application(
        &Application::local(temp.path()).expect("local application"),
        Cursor::new(oversized.into_bytes()),
        &mut out,
    )
    .expect_err("oversized request must fail")
    .to_string();
    assert!(error.contains("exceeds"));
    assert!(out.is_empty());
}

#[test]
fn notifications_are_silent_and_ids_are_preserved() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","id":null,"method":"initialize"}
{"jsonrpc":"2.0","method":"ping"}
{"jsonrpc":"2.0","id":42,"method":"ping"}
{"jsonrpc":"2.0","id":true,"method":"ping"}
"#,
    );
    assert_eq!(values.len(), 3);
    assert!(values.iter().any(|value| value["id"].is_null()));
    assert!(values.iter().any(|value| value["id"] == 42));
    assert_eq!(values[2]["error"]["code"], -32600);
}

#[test]
fn notification_initialize_does_not_advance_handshake() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":1,"method":"tools/list"}
{"jsonrpc":"2.0","method":"initialize"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
"#,
    );
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["error"]["code"], -32002);
    assert_eq!(values[1]["error"]["code"], -32002);
}

#[test]
fn notification_tool_call_is_ignored_before_core_dispatch() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"research.create","arguments":{"request_key":"notification-create","kind":"Q","title":"Must not exist"}}}
"#,
    );
    assert_eq!(values.len(), 1);
    assert!(
        !temp
            .path()
            .join("questions/Q001-must-not-exist.md")
            .exists()
    );
    assert!(
        orbit_research_core::Research::open(temp.path())
            .expect("open corpus")
            .snapshot()
            .expect("read corpus snapshot")
            .records
            .is_empty()
    );
}

#[test]
fn tool_call_delegates_and_wraps_core_errors() {
    let temp = corpus();
    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"research.check","arguments":{}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"research.list","arguments":{}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"research.missing","arguments":{}}}
"#,
    );
    assert!(values[1]["result"]["isError"] == false);
    assert!(values[2]["result"]["isError"] == false);
    let check: Value = serde_json::from_str(
        values[1]["result"]["content"][0]["text"]
            .as_str()
            .expect("check result text"),
    )
    .expect("check result JSON");
    assert_eq!(check["valid"], true);
    assert_eq!(check["record_count"], 0);
    assert!(check.get("records").is_none());
    let list: Value = serde_json::from_str(
        values[2]["result"]["content"][0]["text"]
            .as_str()
            .expect("list result text"),
    )
    .expect("list result JSON");
    assert!(list["revision"].is_string());
    assert!(list["records"].is_array());
    assert!(list["tags"].is_array());
    assert!(values[3]["result"]["isError"] == true);
    assert!(
        values[3]["result"]["content"][0]["text"]
            .as_str()
            .expect("text error content")
            .contains("Unknown research operation")
    );
}

#[test]
fn corpus_diagnostics_are_wrapped_as_mcp_errors_without_raw_git_stderr() {
    let temp = corpus();
    let git = temp.path().join(".git");
    fs::remove_dir_all(git).expect("remove fixture repository");
    let output = Command::new("git")
        .arg("-C")
        .arg(temp.path())
        .args(["init", "-q"])
        .output()
        .expect("initialize unborn fixture repository");
    assert!(output.status.success());

    let values = exchange(
        temp.path(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"research.check","arguments":{}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"research.list","arguments":{}}}
"#,
    );
    for value in &values[1..] {
        assert_eq!(value["result"]["isError"], true);
        let message = value["result"]["content"][0]["text"]
            .as_str()
            .expect("MCP diagnostic text");
        assert!(message.contains("Corpus has no commits"), "{message}");
        assert!(
            message.contains(&temp.path().display().to_string()),
            "{message}"
        );
        assert!(!message.contains("ambiguous argument 'HEAD'"), "{message}");
    }
}
