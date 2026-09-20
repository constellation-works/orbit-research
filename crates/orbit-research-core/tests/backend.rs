use orbit_research_core::backend::{Backend, BackendConfig, Compatibility};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tempfile::TempDir;

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn json_path(path: &Path) -> String {
    serde_json::to_string(&path.display().to_string()).unwrap()
}

fn fixture(mode: &str) -> (TempDir, Backend, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    let marker = temp.path().join("invoked");
    let script = temp.path().join("fake-orbit");
    let other_checkout = temp.path().join("other-checkout");
    fs::create_dir(&other_checkout).unwrap();
    let identity = match mode {
        "wrong-workspace" => format!(
            r#"{{"registered":true,"workspace":{{"id":"ws_other","owner_machine_id":"hm-test"}},"checkout":{{"repo_root":{}}}}}"#,
            json_path(&checkout)
        ),
        "wrong-owner" => format!(
            r#"{{"registered":true,"workspace":{{"id":"ws_test","owner_machine_id":"hm-other"}},"checkout":{{"repo_root":{}}}}}"#,
            json_path(&checkout)
        ),
        "wrong-checkout" => format!(
            r#"{{"registered":true,"workspace":{{"id":"ws_test","owner_machine_id":"hm-test"}},"checkout":{{"repo_root":{}}}}}"#,
            json_path(&other_checkout)
        ),
        _ => format!(
            r#"{{"registered":true,"workspace":{{"id":"ws_test","owner_machine_id":"hm-test"}},"checkout":{{"repo_root":{}}}}}"#,
            json_path(&checkout)
        ),
    };
    let body = format!(
        r#"#!/bin/sh
echo invoked >> {marker}
case "$*" in
  *"--version"*) printf '%s\n' 'orbit 0.23.0' ;;
  *"workspace show"*) printf '%s\n' '{identity}' ;;
  *"orbit.task.list"*) printf '%s\n' '{{"tasks":[{{"id":"T1","tags":["research-request:r1"]}}],"total":1,"truncated":{truncated}}}' ;;
  *"orbit.task.show"*) printf '%s\n' '{{"id":"{task_id}","title":"A task","status":"backlog","job_run_id":null,"artifacts":[]}}' ;;
  *"orbit.task.artifact.get"*) printf '%s\n' '{{"media_type":"text/plain","size":5,"content":"hello"}}' ;;
  *"run show"*) printf '%s\n' '{{"run":{{"run_id":"R1","state":"running"}},"pipeline_state":null,"steps":[]}}' ;;
  *"run ship"*)
    if [ "${{ORBIT_CAPABILITIES-}}" = "operator" ]; then printf '%s\n' '{{"run_id":"R1"}}'; else echo 'capability_denied: operator capability required' >&2; exit 13; fi ;;
  *"run cancel"*) printf '%s\n' '{{"run_id":"R1","state":"cancelled"}}' ;;
  *) printf '%s\n' '{{"id":"T1","status":"backlog"}}' ;;
esac
"#,
        marker = shell_quote(&marker),
        identity = identity,
        task_id = if mode == "wrong-task" { "T2" } else { "T1" },
        truncated = if mode == "truncated" { "true" } else { "false" },
    );
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let hash = format!("{:x}", Sha256::digest(fs::read(&script).unwrap()));
    let backend = Backend::new(
        BackendConfig {
            executable: script.clone(),
            workspace: "ws_test".into(),
            checkout: checkout.clone(),
            owner_machine_id: "hm-test".into(),
        },
        vec![Compatibility {
            binary_sha256: hash,
            version: "orbit 0.23.0".into(),
            platform: std::env::consts::OS.into(),
            operations: vec![
                "observe".into(),
                "task_add".into(),
                "ship".into(),
                "cancel".into(),
            ],
        }],
    )
    .unwrap();
    (temp, backend, marker)
}

#[test]
fn unknown_hash_refuses_before_invocation() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    let marker = temp.path().join("invoked");
    let script = temp.path().join("fake-orbit");
    fs::write(
        &script,
        format!("#!/bin/sh\necho invoked >> {}\n", shell_quote(&marker)),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let backend = Backend::new(
        BackendConfig {
            executable: script,
            workspace: "ws_test".into(),
            checkout,
            owner_machine_id: "hm-test".into(),
        },
        vec![Compatibility {
            binary_sha256: "00".into(),
            version: "orbit 0.23.0".into(),
            platform: std::env::consts::OS.into(),
            operations: vec!["observe".into()],
        }],
    )
    .unwrap();
    let error = backend.inspect().unwrap_err().to_string();
    assert!(error.contains("not certified for observe"));
    assert!(!marker.exists(), "unknown binaries must not be invoked");
}

#[test]
fn explicit_workspace_owner_and_checkout_mismatches_refuse() {
    for mode in ["wrong-workspace", "wrong-owner", "wrong-checkout"] {
        let (_temp, backend, _marker) = fixture(mode);
        let error = backend.inspect().unwrap_err().to_string();
        assert!(
            error.contains("different workspace or owner") || error.contains("checkout differs"),
            "{mode}: {error}"
        );
    }
}

#[test]
fn read_observations_and_artifact_shape_are_checked() {
    let (_temp, backend, _marker) = fixture("valid");
    let task = backend.task("T1").unwrap();
    assert_eq!(task["id"], "T1");
    assert_eq!(task["status"], "backlog");
    assert_eq!(backend.run("R1").unwrap()["run_id"], "R1");
    assert_eq!(
        backend.artifact("T1", "notes.txt").unwrap()["content"],
        "hello"
    );
}

#[test]
fn task_id_mismatch_refuses() {
    let (_temp, backend, _marker) = fixture("wrong-task");
    let error = backend.task("T1").unwrap_err().to_string();
    assert!(error.contains("wrong task"), "{error}");
}

#[test]
fn agent_only_dispatch_refusal_is_preserved_without_operator_override() {
    let (_temp, backend, _marker) = fixture("valid");
    let error = backend.dispatch("T1", "main").unwrap_err().to_string();
    assert!(error.contains("capability_denied"), "{error}");
}

#[test]
fn truncated_task_reconciliation_refuses_retry() {
    let (_temp, backend, _marker) = fixture("truncated");
    let error = backend
        .correlated_tasks("research-request:r1")
        .unwrap_err()
        .to_string();
    assert!(error.contains("truncated"));
}
