//! Exercise request validation and resource limits through the real exec backend.
use serde_json::{Value, json};
use std::io::{Seek, Write};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

const REQUEST_LIMIT: usize = 1024 * 1024;

fn run(request: &[u8]) -> (TempDir, Output) {
    let root = TempDir::new().expect("isolated plugin root");
    let mut input = tempfile::tempfile().expect("request file");
    input.write_all(request).expect("write request");
    input.rewind().expect("rewind request");
    let output = Command::new(env!("CARGO_BIN_EXE_orbit-research"))
        .arg("orbit-tool")
        .env_clear()
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .current_dir(root.path())
        .stdin(Stdio::from(input))
        .output()
        .expect("run plugin executable");
    assert!(
        output.status.success(),
        "structured errors exit zero: {output:?}"
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    (root, output)
}

fn padded_version_request(bytes: usize) -> Vec<u8> {
    let mut request =
        br#"{"schema_version":1,"tool":"orbit.research.version","input":{}}"#.to_vec();
    request.resize(bytes, b' ');
    request
}

#[test]
fn plugin_request_at_the_byte_limit_is_accepted() {
    let (_, output) = run(&padded_version_request(REQUEST_LIMIT));
    let response: Value = serde_json::from_slice(&output.stdout).expect("plugin envelope");
    assert_eq!(response["ok"], true, "{response}");
}

#[test]
fn oversized_plugin_request_is_refused_before_dispatch() {
    let (root, output) = run(&padded_version_request(REQUEST_LIMIT + 1));
    let response: Value = serde_json::from_slice(&output.stdout).expect("plugin envelope");
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["error"]["code"], "invalid_request");
    assert_eq!(response["error"]["retryable"], false);
    let message = response["error"]["message"]
        .as_str()
        .expect("actionable error");
    assert!(message.contains("1048576"), "{message}");
    assert!(message.contains("reduce"), "{message}");
    assert_eq!(
        std::fs::read_dir(root.path()).expect("list root").count(),
        0
    );
}

#[test]
fn version_enforces_the_published_empty_object_schema() {
    for input in [
        json!({"ignored": true}),
        json!(null),
        json!([]),
        json!(false),
        json!("ignored"),
    ] {
        let request =
            json!({"schema_version": 1, "tool": "orbit.research.version", "input": input});
        let (_, output) = run(&serde_json::to_vec(&request).expect("request JSON"));
        let response: Value = serde_json::from_slice(&output.stdout).expect("plugin envelope");
        assert_eq!(response["ok"], false, "input {input}: {response}");
        assert_eq!(response["error"]["code"], "invalid_request", "{response}");
        assert_eq!(response["error"]["retryable"], false);
    }
}

#[test]
fn every_tool_requires_object_input_before_any_workspace_or_host_access() {
    for tool in [
        "list", "show", "check", "version", "plan", "link", "validate", "accept",
    ] {
        for input in [json!(null), json!([]), json!(false), json!("ignored")] {
            let request = json!({"tool": tool, "input": input});
            let (root, output) = run(&serde_json::to_vec(&request).expect("request JSON"));
            let response: Value = serde_json::from_slice(&output.stdout).expect("plugin envelope");
            assert_eq!(
                response["error"]["code"], "invalid_request",
                "{tool}, {input}: {response}"
            );
            assert_eq!(
                std::fs::read_dir(root.path()).expect("list root").count(),
                0
            );
        }
    }
}
