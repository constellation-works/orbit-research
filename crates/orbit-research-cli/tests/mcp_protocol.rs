use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "MCP fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "MCP fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn exchange(requests: &[Value]) -> Vec<Value> {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let corpus = temp.path().join("corpus");
    let initialized = command()
        .args(["workspace", "init"])
        .arg(&corpus)
        .output()
        .expect("initialize corpus");
    assert!(initialized.status.success(), "{initialized:?}");
    let mut child = command()
        .args(["mcp", "--corpus"])
        .arg(&corpus)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    for request in requests {
        serde_json::to_writer(&mut input, request).expect("write MCP request");
        writeln!(input).expect("terminate MCP request");
    }
    drop(input);
    let output = child.wait_with_output().expect("finish MCP server");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

#[test]
fn request_ids_receive_replies_even_for_notification_method_names() {
    let responses = exchange(&[
        json!({"jsonrpc":"2.0", "id":1, "method":"initialize"}),
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0", "id":"reply-required", "method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0", "id":3, "method":"ping"}),
    ]);
    assert_eq!(responses.len(), 3, "{responses:?}");
    assert_eq!(responses[1]["id"], "reply-required");
    assert_eq!(responses[1]["error"]["code"], -32601);
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"], json!({}));
}

#[test]
fn malformed_envelopes_and_tool_arguments_are_protocol_errors() {
    let responses = exchange(&[
        json!({"jsonrpc":"2.0", "id":1, "method":"initialize"}),
        json!({"jsonrpc":"2.0", "id":2, "method":"ping", "params":4}),
        json!({"jsonrpc":"2.0", "id":3, "method":"tools/list", "params":[]}),
        json!({"jsonrpc":"2.0", "id":4, "method":"tools/call", "params":{"name":"research.list", "arguments":[]}}),
        json!({"jsonrpc":"2.0", "id":5, "method":"tools/call", "params":{"name":"research.missing"}}),
        json!({"jsonrpc":"2.0", "id":6, "method":"tools/call", "params":{"name":"research.show", "arguments":{"id":"malformed"}}}),
        json!({"jsonrpc":"2.0", "id":7, "method":"tools/call", "params":{"name":"research.list", "arguments":{"corpus":"/outside"}}}),
        json!({"jsonrpc":"2.0", "id":8, "method":"tools/call", "params":{"name":"research.show", "arguments":{"id":"Q999"}}}),
        json!({"jsonrpc":"2.0", "id":9, "method":"ping"}),
    ]);
    assert_eq!(responses.len(), 9, "{responses:?}");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["error"]["code"], -32600);
    for (response, id) in responses[2..7].iter().zip(3..=7) {
        assert_eq!(response["id"], id, "{response}");
        assert_eq!(response["error"]["code"], -32602, "{response}");
        assert!(response.get("result").is_none(), "{response}");
    }
    assert_eq!(responses[7]["result"]["isError"], true);
    assert!(responses[7].get("error").is_none());
    assert_eq!(responses[8]["result"], json!({}));
}

#[test]
fn invalid_requests_preserve_known_ids_and_use_null_for_invalid_or_absent_ids() {
    let responses = exchange(&[
        json!({"jsonrpc":"1.0", "id":"known", "method":"ping"}),
        json!({"jsonrpc":"2.0", "id":17, "method":4}),
        json!({"jsonrpc":"2.0", "id":null, "method":4}),
        json!({"jsonrpc":"2.0", "id":true, "method":"ping"}),
        json!({"jsonrpc":"2.0", "method":4}),
    ]);
    assert_eq!(responses.len(), 5, "{responses:?}");
    for (response, id) in responses.iter().zip([
        json!("known"),
        json!(17),
        Value::Null,
        Value::Null,
        Value::Null,
    ]) {
        assert_eq!(response["id"], id, "{response}");
        assert_eq!(response["error"]["code"], -32600, "{response}");
    }
}
