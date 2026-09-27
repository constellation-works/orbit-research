//! Orbit plugin `exec` backend transport (`orbit-tool`, invoked as
//! `orbit-research orbit-tool`). One process per call: read the host's single
//! stdin JSON request, dispatch through Core, write the single stdout JSON
//! reply. All research operations delegate to Core, as MCP does; this
//! transport speaks the plugin envelope instead of JSON-RPC and always exits
//! `0`, carrying failure in `ok:false` rather than a process exit code.
use orbit_research_core::{Error, Result, api::Application, application::Operation};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Map a plugin tool call to a Core operation. The host may send the bare
/// verb, the local-development namespace (`research.list`) or the verified
/// first-party namespace (`orbit.research.list`); only the trailing verb is
/// wire contract here, so all three resolve the same way.
fn operation_for_verb(verb: &str) -> Option<Operation> {
    match verb {
        "list" => Some(Operation::List),
        "show" => Some(Operation::Show),
        "check" => Some(Operation::Check),
        "plan" => Some(Operation::Plan),
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
        Error::Refused(_) => "refused",
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

/// A reference to an Orbit task, as returned by `orbit.task.list`/`orbit.task.add`.
pub(crate) struct TaskRef {
    pub(crate) id: String,
}

/// The host callbacks `link` needs. Production calls spawn `orbit tool run`
/// (the only way a sandboxed `exec` backend reaches Orbit); tests substitute
/// an in-memory fake so goldens don't need a real Orbit installation.
pub(crate) trait TaskHost {
    fn list_by_tag(&self, workspace: &str, tag: &str) -> Result<Vec<TaskRef>>;
    fn create(
        &self,
        workspace: &str,
        tag: &str,
        title: &str,
        description: &str,
        acceptance_criteria: &[String],
        context_files: &[String],
    ) -> Result<TaskRef>;
}

/// Spawns the host's own `orbit` binary, exactly as a person would run
/// `orbit tool run <name> --input <json>`. Reachable without a
/// `requires.programs` declaration: a callback to a tool granted under
/// `permissions.orbit_tools` is host-mediated, not an arbitrary subprocess.
pub(crate) struct OrbitCliTaskHost;

fn orbit_binary() -> String {
    std::env::var("ORBIT_BIN").unwrap_or_else(|_| "orbit".into())
}

fn run_orbit_tool(name: &str, input: &Value) -> Result<Value> {
    let output = Command::new(orbit_binary())
        .args(["tool", "run", name, "--input", &input.to_string()])
        .output()
        .map_err(|error| {
            Error::Internal(format!("failed to invoke `orbit tool run {name}`: {error}"))
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!(
            "`orbit tool run {name}` failed: {}",
            message.trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        Error::Internal(format!(
            "`orbit tool run {name}` returned non-JSON output: {error}"
        ))
    })
}

impl TaskHost for OrbitCliTaskHost {
    fn list_by_tag(&self, workspace: &str, tag: &str) -> Result<Vec<TaskRef>> {
        let output = run_orbit_tool(
            "orbit.task.list",
            &json!({"workspace": workspace, "tag": tag}),
        )?;
        let tasks = output
            .get("tasks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tasks
            .iter()
            .filter_map(|task| task.get("id").and_then(Value::as_str))
            .map(|id| TaskRef { id: id.into() })
            .collect())
    }

    fn create(
        &self,
        workspace: &str,
        tag: &str,
        title: &str,
        description: &str,
        acceptance_criteria: &[String],
        context_files: &[String],
    ) -> Result<TaskRef> {
        let output = run_orbit_tool(
            "orbit.task.add",
            &json!({
                "workspace": workspace,
                "title": title,
                "description": description,
                "complexity": "medium",
                "acceptance_criteria": acceptance_criteria,
                "context_files": context_files,
                "allow_missing_context": true,
                "tags": [tag],
            }),
        )?;
        let id = output
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Internal("orbit.task.add did not return a task id".into()))?;
        Ok(TaskRef { id: id.into() })
    }
}

// `link`'s own input contract. Not a Core `Operation`: only this transport
// can reach the `orbit.task.add`/`orbit.task.list` callbacks, and Core never
// shells out to Orbit (see ARCHITECTURE.md). `context_files` is deliberately
// absent: `link` derives it itself from the reserved research item, so the
// acceptance invariant ("context_files names the reserved R") cannot be
// bypassed by caller input. A plain (non-doc) comment: schemars would lift a
// doc comment into the schema's `description`, which schemas/link.request.json
// (checked for drift in src/tests/plugin.rs) does not carry.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkInput {
    #[schemars(regex(pattern = "^R[0-9]{3}$"))]
    research_id: String,
    #[schemars(length(min = 1, max = 256))]
    request_key: String,
    #[schemars(length(min = 1))]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
}

fn link(workspace_root: &Path, input: Value, host: &dyn TaskHost) -> Result<Value> {
    let input: LinkInput =
        serde_json::from_value(input).map_err(|error| Error::InvalidInput(error.to_string()))?;
    let app = Application::local(workspace_root)?;
    let preparation = app.link_intent(&input.request_key, &input.research_id)?;
    let workspace = workspace_root.to_string_lossy();
    let tag = &preparation.link.correlation_tag;
    let matches = host.list_by_tag(&workspace, tag)?;
    match matches.len() {
        1 => {
            let task_id = &matches[0].id;
            app.link_confirm(&input.request_key, task_id)?;
            Ok(json!({
                "research_id": input.research_id,
                "task_id": task_id,
                "created": false,
            }))
        }
        0 if preparation.is_new => {
            let task = host.create(
                &workspace,
                tag,
                &input.title,
                &input.description,
                &input.acceptance_criteria,
                &preparation.context_files,
            )?;
            app.link_confirm(&input.request_key, &task.id)?;
            Ok(json!({
                "research_id": input.research_id,
                "task_id": task.id,
                "created": true,
            }))
        }
        0 => Err(Error::Conflict(format!(
            "A prior link attempt for this request key was recorded, but no Orbit task tagged `{tag}` exists; resolve manually before retrying"
        ))),
        count => Err(Error::Conflict(format!(
            "{count} Orbit tasks are tagged `{tag}`; expected exactly one"
        ))),
    }
}

fn link_output(workspace_root: &Path, input: Value, host: &dyn TaskHost) -> Value {
    match link(workspace_root, input, host) {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => error_envelope(error_code(&error), error.to_string()),
    }
}

fn handle(request_bytes: &[u8], host: &dyn TaskHost) -> Value {
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
    let workspace_root = request
        .pointer("/context/workspace_root")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let Some(workspace_root) = workspace_root else {
        return error_envelope("workspace_required", "This tool requires a bound workspace");
    };
    let input = request.get("input").cloned().unwrap_or(json!({}));
    let input = if input.is_null() { json!({}) } else { input };
    if verb(tool) == "link" {
        return link_output(&workspace_root, input, host);
    }
    let Some(operation) = operation_for_verb(verb(tool)) else {
        return error_envelope(
            "invalid_request",
            format!("Unknown research plugin tool: {tool}"),
        );
    };
    match Application::local(&workspace_root).and_then(|app| app.execute(operation, input)) {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => error_envelope(error_code(&error), error.to_string()),
    }
}

/// Serve exactly one plugin tool call: all of stdin is one request, all of
/// stdout is one reply. Only an I/O or encoding failure while handling the
/// process streams is a Rust `Err`; a bad request or a Core refusal is
/// reported as `ok:false` in the reply, never a nonzero exit.
pub fn serve_plugin_tool_call(reader: impl Read, writer: impl Write) -> Result<()> {
    serve_plugin_tool_call_with_host(reader, writer, &OrbitCliTaskHost)
}

pub(crate) fn serve_plugin_tool_call_with_host(
    mut reader: impl Read,
    mut writer: impl Write,
    host: &dyn TaskHost,
) -> Result<()> {
    let mut request_bytes = Vec::new();
    reader.read_to_end(&mut request_bytes)?;
    let response = handle(&request_bytes, host);
    serde_json::to_writer(&mut writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
