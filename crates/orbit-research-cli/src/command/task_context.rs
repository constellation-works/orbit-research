//! Explicit Orbit task observation through the registered executable.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use crate::output::Invalid;

pub(crate) fn execute(
    orbit_root: &Path,
    host: &str,
    workspace: &str,
    task: &str,
    run: &str,
    orbit_executable: &Path,
) -> Result<(Value, u8), Invalid> {
    if !orbit_root.is_absolute() {
        return Err("explicit absolute Orbit authority root required"
            .to_owned()
            .into());
    }
    if [host, workspace, task, run]
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err("explicit host/workspace/task/run required"
            .to_owned()
            .into());
    }
    let request = json!({"id": task, "workspace": workspace, "model": "codex"});
    let output = Command::new(orbit_executable)
        .args(["tool", "run", "orbit.task.show", "--root"])
        .arg(orbit_root)
        .args(["--input", &request.to_string()])
        .output()
        .map_err(|error| format!("{}: {error}", orbit_executable.display()))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("Orbit task lookup exited with {}", output.status)
        } else {
            detail
        }
        .into());
    }
    let mut value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid Orbit response: {error}"))?;
    if let Some(result) = value.get("result").filter(|item| item.is_object()) {
        value = result.clone();
    }
    if value.get("id").and_then(Value::as_str) != Some(task) {
        return Err("Orbit response does not identify assigned task"
            .to_owned()
            .into());
    }
    if value
        .get("workspace")
        .and_then(|owner| owner.get("id"))
        .and_then(Value::as_str)
        != Some(workspace)
    {
        return Err("Orbit response workspace differs from explicit authority"
            .to_owned()
            .into());
    }
    if value.get("terminal").and_then(Value::as_bool) == Some(true)
        || matches!(
            value.get("status").and_then(Value::as_str),
            Some("done" | "rejected")
        )
    {
        return Err("assigned task is terminal".to_owned().into());
    }
    Ok((
        json!({"task":value,"orbit_link":{"host":host,"workspace":workspace,"task":task,"run":run}}),
        0,
    ))
}
