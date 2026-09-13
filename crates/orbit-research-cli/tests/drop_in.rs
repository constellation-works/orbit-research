//! CLI acceptance after the Python package was retired.
//!
//! Fixture generators under `examples/` are host Python scripts that do not import
//! `orbit_research`. They either write synthetic source trees or drive this binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn python() -> String {
    std::env::var("ORBIT_RESEARCH_PYTHON").unwrap_or_else(|_| "python3".to_owned())
}

fn run(mut command: Command) -> Output {
    let description = format!("{command:?}");
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{description}: {error}"));
    assert!(
        output.status.success(),
        "{description}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn rust(args: &[&str]) -> Value {
    let mut command = Command::new(BINARY);
    command.args(args);
    serde_json::from_slice(&run(command).stdout).expect("Rust stdout JSON")
}

fn json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
}

fn record_structure(records: &Value) -> Vec<(String, u64)> {
    let mut structure = records
        .as_array()
        .expect("records array")
        .iter()
        .map(|record| {
            let revision = record["revision_id"].as_str().expect("record revision");
            assert!(
                revision.starts_with("sha256:") && revision.len() == "sha256:".len() + 64,
                "{revision}"
            );
            (
                record["id"].as_str().expect("record id").to_owned(),
                record["authorship"]["sequence"].as_u64().unwrap_or(0),
            )
        })
        .collect::<Vec<_>>();
    structure.sort();
    structure
}

fn owner_record_structure(root: &Path) -> Vec<(String, u64)> {
    let mut records = Vec::new();
    for namespace in ["physics-fixture", "parallax-fixture"] {
        let directory = root.join(namespace).join("research/records");
        let mut paths = fs::read_dir(directory)
            .expect("record directory")
            .map(|entry| entry.expect("record entry").path())
            .collect::<Vec<_>>();
        paths.sort();
        records.extend(paths.into_iter().map(json));
    }
    record_structure(&Value::Array(records))
}

#[test]
fn resource_and_explicit_task_context_match_protocol() {
    let resource = rust(&["resource", "--version", "1"]);
    assert_eq!(resource["version"], 1);
    assert!(
        resource["skill"]
            .as_str()
            .expect("skill text")
            .starts_with("---\nname: orbit-research-native\n")
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    let executable = tmp.path().join("orbit-fixture.py");
    fs::write(
        &executable,
        r#"#!/usr/bin/env python3
import json, sys
args=sys.argv[1:]
request=json.loads(args[args.index('--input')+1])
assert args[:3] == ['tool','run','orbit.task.show']
print(json.dumps({'id':request['id'],'workspace':{'id':request['workspace']},'status':'in-progress'}))
"#,
    )
    .expect("write fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let context = rust(&[
        "task-context",
        "--orbit-root",
        tmp.path().to_str().expect("UTF-8 root"),
        "--host",
        "fixture-host",
        "--workspace",
        "ws_fixture",
        "--task",
        "fixture-task",
        "--run",
        "fixture-run",
        "--orbit-executable",
        executable.to_str().expect("UTF-8 executable"),
    ]);
    assert_eq!(
        context["orbit_link"],
        json!({
            "host": "fixture-host",
            "workspace": "ws_fixture",
            "task": "fixture-task",
            "run": "fixture-run"
        })
    );
}

#[test]
fn synthetic_four_owner_imports_preserve_counts_and_candidate_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let sources = tmp.path().join("sources");
    let mut fixture = Command::new(python());
    fixture
        .current_dir(repo_root())
        .arg("examples/make_fixture_sources.py")
        .arg(&sources);
    run(fixture);

    for adapter in ["principia", "parallax", "orrery", "astrolabe"] {
        let source = sources.join(adapter);
        let actual = rust(&[
            "import",
            adapter,
            "--source-root",
            source.to_str().expect("UTF-8 source"),
            "--repository",
            adapter,
            "--dry-run",
        ]);
        let discovered = actual["counts"]["discovered"].as_u64().expect("discovered");
        let mapped = actual["counts"]["mapped"].as_u64().expect("mapped");
        let exceptions = actual["counts"]["exceptions"].as_u64().expect("exceptions");
        assert!(discovered > 0, "{adapter} discovered nothing");
        assert_eq!(discovered, mapped + exceptions, "{adapter} counts");
        let ids = actual["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .map(|candidate| candidate["id"].as_str().expect("candidate id").to_owned())
            .collect::<Vec<_>>();
        assert!(!ids.is_empty(), "{adapter} candidate ids");
        if adapter == "principia" {
            let mut sorted = ids;
            sorted.sort();
            assert_eq!(
                sorted,
                vec![
                    "urn:research:principia:assessment:control-null%3Alegacy-verdict",
                    "urn:research:principia:claim:control-null",
                    "urn:research:principia:program:calibration",
                    "urn:research:principia:protocol:control",
                ]
            );
        }
    }
}

#[test]
fn native_workflow_produces_digest_stable_appends_traces_and_exports() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("owners");
    let mut command = Command::new(python());
    command
        .current_dir(repo_root())
        .arg("examples/native_workflow.py")
        .arg(&root)
        .env("ORBIT_RESEARCH_BINARY", BINARY);
    run(command);

    let structure = owner_record_structure(&root);
    assert!(!structure.is_empty(), "native workflow wrote no records");
    for namespace in ["physics-fixture", "parallax-fixture"] {
        let export = json(root.join(namespace).join("export.json"));
        assert!(
            !record_structure(&export["records"]).is_empty(),
            "{namespace} export"
        );
        let trace = json(root.join(namespace).join("trace.json"));
        assert!(
            !record_structure(&trace["records"]).is_empty(),
            "{namespace} trace"
        );
        let validated = rust(&[
            "validate",
            root.join(namespace)
                .join("export.json")
                .to_str()
                .expect("UTF-8 export"),
        ]);
        assert_eq!(validated["valid"], true, "{namespace} validate");
    }
}
