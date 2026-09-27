//! Orbit plugin `exec` backend transport (`orbit-tool`, invoked as
//! `orbit-research orbit-tool`). One process per call: read the host's single
//! stdin JSON request, dispatch through Core, write the single stdout JSON
//! reply. All research operations delegate to Core, as MCP does; this
//! transport speaks the plugin envelope instead of JSON-RPC and always exits
//! `0`, carrying failure in `ok:false` rather than a process exit code.
use orbit_research_core::{Error, Result, api::Application, application::Operation};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Map a plugin tool call to a Core operation. The host may send the bare
/// verb, the local-development namespace (`research.list`) or the verified
/// first-party namespace (`orbit.research.list`); only the trailing verb is
/// wire contract here, so all three resolve the same way.
fn operation_for_verb(verb: &str) -> Option<Operation> {
    match verb {
        "list" => Some(Operation::List),
        "show" => Some(Operation::Show),
        "check" => Some(Operation::Check),
        _ => None,
    }
}

fn verb(tool: &str) -> &str {
    tool.rsplit('.').next().unwrap_or(tool)
}

fn error_code(error: &Error) -> &'static str {
    match error {
        Error::InvalidInput(_) => "invalid_request",
        Error::NotFound(_) => "record_not_found",
        Error::Conflict(_) => "conflict",
        Error::Invalid(_) => "corpus_unavailable",
        Error::Internal(_) => "internal",
        Error::Io(_) => "io_error",
        Error::Json(_) | Error::Yaml(_) => "internal",
    }
}

fn error_envelope(code: &str, message: impl Into<String>) -> Value {
    json!({"ok": false, "error": {"code": code, "message": message.into(), "retryable": false}})
}

/// The plugin protocol's `version` tool reports Core's own version
/// unconditionally: a corpus-independent health signal, so it still answers
/// when the bound workspace has no valid corpus.
fn version_output() -> Value {
    json!({"ok": true, "output": {"core_version": orbit_research_core::VERSION}})
}

fn handle(request_bytes: &[u8]) -> Value {
    let request: Value = match serde_json::from_slice(request_bytes) {
        Ok(request) => request,
        Err(error) => {
            return error_envelope(
                "invalid_request",
                format!("Malformed plugin tool call: {error}"),
            );
        }
    };
    let Some(tool) = request.get("tool").and_then(Value::as_str) else {
        return error_envelope("invalid_request", "Missing plugin tool call `tool` name");
    };
    if verb(tool) == "version" {
        return version_output();
    }
    let Some(operation) = operation_for_verb(verb(tool)) else {
        return error_envelope(
            "invalid_request",
            format!("Unknown research plugin tool: {tool}"),
        );
    };
    let workspace_root = request
        .pointer("/context/workspace_root")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let Some(workspace_root) = workspace_root else {
        return error_envelope("workspace_required", "This tool requires a bound workspace");
    };
    let input = request.get("input").cloned().unwrap_or(json!({}));
    let input = if input.is_null() { json!({}) } else { input };
    match Application::local(&workspace_root).and_then(|app| app.execute(operation, input)) {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => error_envelope(error_code(&error), error.to_string()),
    }
}

/// Serve exactly one plugin tool call: all of stdin is one request, all of
/// stdout is one reply. Only an I/O or encoding failure while handling the
/// process streams is a Rust `Err`; a bad request or a Core refusal is
/// reported as `ok:false` in the reply, never a nonzero exit.
pub fn serve_plugin_tool_call(mut reader: impl Read, mut writer: impl Write) -> Result<()> {
    let mut request_bytes = Vec::new();
    reader.read_to_end(&mut request_bytes)?;
    let response = handle(&request_bytes);
    serde_json::to_writer(&mut writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
