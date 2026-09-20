//! MCP transport. All research operations delegate to Core.
use orbit_research_core::{
    Error, Result,
    api::{Application, tools},
};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;

const MAX_REQUEST_BYTES: usize = 128 * 1024;

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}
fn valid_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.is_number()
}

/// Read one newline-delimited request without retaining more than the protocol limit.
fn bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            if bytes.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(chunk.len(), |position| position + 1);
        let room = MAX_REQUEST_BYTES.saturating_sub(bytes.len());
        if take > room {
            oversized = true;
            bytes.extend_from_slice(&chunk[..room]);
        } else {
            bytes.extend_from_slice(&chunk[..take]);
        }
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    if oversized || bytes.len() > MAX_REQUEST_BYTES {
        return Err(Error::Invalid(format!(
            "MCP request exceeds {MAX_REQUEST_BYTES} bytes"
        )));
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Error::Invalid("MCP request is not UTF-8".into()))
}

fn write_response<W: Write>(writer: &mut W, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, &value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// MCP stdio JSON-RPC, pinned to the 2024-11-05 tools protocol.
/// Corpus scope is chosen at process startup, never supplied by tool arguments.
pub fn serve_mcp(root: &Path, mut reader: impl BufRead, mut writer: impl Write) -> Result<()> {
    let application = Application::local(root)?;
    serve_mcp_application(&application, &mut reader, &mut writer)
}

/// Serve MCP against one process-scoped Core application. The application owns
/// the fixed corpus and optional backend; request arguments cannot replace it.
pub fn serve_mcp_application(
    application: &Application,
    mut reader: impl BufRead,
    mut writer: impl Write,
) -> Result<()> {
    let mut initialized = false;
    while let Some(line) = bounded_line(&mut reader)? {
        let request = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(_) => {
                write_response(
                    &mut writer,
                    error_response(Value::Null, -32700, "Invalid JSON"),
                )?;
                continue;
            }
        };
        let object = match request.as_object() {
            Some(object) => object,
            None => {
                write_response(
                    &mut writer,
                    error_response(Value::Null, -32600, "Invalid Request"),
                )?;
                continue;
            }
        };
        let id_present = object.contains_key("id");
        let id = object.get("id").cloned().unwrap_or(Value::Null);
        let notification = !id_present;
        let valid_shape = object.get("jsonrpc") == Some(&Value::String("2.0".into()))
            && object.get("method").and_then(Value::as_str).is_some()
            && object.get("id").is_none_or(valid_id);
        let invalid = !valid_shape;
        if invalid {
            write_response(
                &mut writer,
                error_response(Value::Null, -32600, "Invalid Request"),
            )?;
            continue;
        }
        let method = object["method"].as_str().expect("validated method");
        if method == "notifications/initialized" {
            continue;
        }
        if method == "initialize" {
            if object
                .get("params")
                .is_some_and(|params| !params.is_object())
            {
                if !notification {
                    write_response(
                        &mut writer,
                        error_response(id, -32602, "initialize params must be an object"),
                    )?;
                }
                continue;
            }
            if !notification {
                initialized = true;
                write_response(
                    &mut writer,
                    response(
                        id,
                        json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"orbit-research","version":"0.1.0"}}),
                    ),
                )?;
            }
            continue;
        }
        if !initialized {
            if !notification {
                write_response(
                    &mut writer,
                    error_response(id, -32002, "MCP session is not initialized"),
                )?;
            }
            continue;
        }
        let result = match method {
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools":tools()})),
            "tools/call" => {
                // A notification has no durable request identity. Ignore it
                // before argument parsing or Core dispatch so it cannot mutate.
                if notification {
                    continue;
                }
                let params = match object.get("params").and_then(Value::as_object) {
                    Some(params) => params,
                    None => {
                        if !notification {
                            write_response(
                                &mut writer,
                                error_response(id, -32602, "tools/call params must be an object"),
                            )?;
                        }
                        continue;
                    }
                };
                let name = match params.get("name").and_then(Value::as_str) {
                    Some(name) => name,
                    None => {
                        if !notification {
                            write_response(
                                &mut writer,
                                error_response(id, -32602, "Missing tool name"),
                            )?;
                        }
                        continue;
                    }
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !arguments.is_object() {
                    Err(Error::Invalid(
                        "tools/call arguments must be an object".into(),
                    ))
                } else {
                    application.call(name, arguments).map(|value| json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})).map_err(|error| Error::Invalid(error.to_string()))
                }
            }
            _ => {
                if !notification {
                    write_response(&mut writer, error_response(id, -32601, "Method not found"))?;
                }
                continue;
            }
        };
        if notification {
            continue;
        }
        let envelope = match result {
            Ok(value) => response(id, value),
            Err(error) => response(
                id,
                json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}),
            ),
        };
        write_response(&mut writer, envelope)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Cursor, process::Command};
    use tempfile::TempDir;
    const SCHEMA: &[u8] = include_bytes!("../../orbit-research-core/tests/fixtures/schema.json");
    fn corpus() -> TempDir {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("_scripts")).unwrap();
        fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
        for dir in ["questions", "hypotheses", "theories", "research"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        let run = |args: &[&str]| {
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
        serve_mcp(root, Cursor::new(input.as_bytes()), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn exchange_application(root: &Path, input: &str) -> Vec<Value> {
        let application = Application::local(root).unwrap();
        let mut output = Vec::new();
        serve_mcp_application(&application, Cursor::new(input.as_bytes()), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
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
                .unwrap()
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
            "x".repeat(MAX_REQUEST_BYTES)
        );
        let mut out = Vec::new();
        let error = serve_mcp(temp.path(), Cursor::new(oversized.into_bytes()), &mut out)
            .unwrap_err()
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
        let values = exchange_application(
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
                .unwrap()
                .snapshot()
                .unwrap()
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
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"research.list","arguments":{}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"research.missing","arguments":{}}}
"#,
        );
        assert!(values[1]["result"]["isError"] == false);
        assert!(values[2]["result"]["isError"] == true);
        assert!(
            values[2]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Unknown research operation")
        );
    }
}
