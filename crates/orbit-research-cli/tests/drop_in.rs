//! Executable Python 0.3/Rust drop-in evidence.
//!
//! The Python fixtures are the oracle. This suite covers the full synthetic import,
//! native-workflow, and browser examples plus their validate/reconcile paths. The only
//! Python unittest areas outside this cross-process subset are negative unit-level
//! mutations and owner-specific artifact-resolver/Parallax edge cases; those invariants
//! are ported as focused tests in the contract, owner, import, and index crates.

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

fn python_command() -> Command {
    let root = repo_root();
    let mut command = Command::new(python());
    command
        .current_dir(&root)
        .env("PYTHONPATH", root.join("src"));
    command
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

fn python_cli(args: &[&str]) -> Value {
    let mut command = python_command();
    command.args(["-m", "orbit_research"]).args(args);
    serde_json::from_slice(&run(command).stdout).expect("Python stdout JSON")
}

fn json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
}

fn record_identity(records: &Value) -> Vec<(String, String, u64)> {
    let mut identity = records
        .as_array()
        .expect("records array")
        .iter()
        .map(|record| {
            (
                record["id"].as_str().expect("record id").to_owned(),
                record["revision_id"]
                    .as_str()
                    .expect("record revision")
                    .to_owned(),
                record["authorship"]["sequence"].as_u64().unwrap_or(0),
            )
        })
        .collect::<Vec<_>>();
    identity.sort();
    identity
}

fn record_structure(records: &Value) -> Vec<(String, u64)> {
    let identity = record_identity(records);
    assert!(identity.iter().all(|(_, revision, _)| {
        revision.starts_with("sha256:") && revision.len() == "sha256:".len() + 64
    }));
    let mut structure = identity
        .into_iter()
        .map(|(id, _, sequence)| (id, sequence))
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

fn projection_identity(records: &Value) -> Vec<(String, String, String)> {
    records
        .as_array()
        .expect("projected records")
        .iter()
        .map(|projected| {
            (
                projected["key"]
                    .as_str()
                    .expect("projection key")
                    .to_owned(),
                projected["record"]["id"]
                    .as_str()
                    .expect("record id")
                    .to_owned(),
                projected["record"]["revision_id"]
                    .as_str()
                    .expect("record revision")
                    .to_owned(),
            )
        })
        .collect()
}

#[test]
fn resource_and_explicit_task_context_match_python_protocol() {
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
fn synthetic_four_owner_imports_match_python_counts_and_candidate_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let sources = tmp.path().join("sources");
    let mut fixture = python_command();
    fixture
        .arg("examples/make_fixture_sources.py")
        .arg(&sources);
    run(fixture);

    for adapter in ["principia", "parallax", "orrery", "astrolabe"] {
        let source = sources.join(adapter);
        let args = [
            "import",
            adapter,
            "--source-root",
            source.to_str().expect("UTF-8 source"),
            "--repository",
            adapter,
            "--dry-run",
        ];
        let expected = python_cli(&args);
        let actual = rust(&args);
        assert_eq!(actual["counts"], expected["counts"], "{adapter} counts");
        let ids = |report: &Value| {
            report["candidates"]
                .as_array()
                .expect("candidates")
                .iter()
                .map(|candidate| candidate["id"].as_str().expect("candidate id").to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&actual), ids(&expected), "{adapter} candidate ids");
    }
}

#[test]
fn native_workflow_matches_python_record_revision_and_sequence_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let python_root = tmp.path().join("python");
    let rust_root = tmp.path().join("rust");

    let mut expected = python_command();
    expected
        .arg("examples/native_workflow.py")
        .arg(&python_root);
    run(expected);

    let source = fs::read_to_string(repo_root().join("examples/native_workflow.py"))
        .expect("native workflow source");
    let adapted = source
        .replacen("import argparse", "import argparse\nimport os", 1)
        .replace(
            "argv=[sys.executable,'-m','orbit_research',command]",
            "argv=[os.environ['ORBIT_RESEARCH_BINARY'],command]",
        );
    assert_ne!(adapted, source, "Rust fixture adapter replacement");
    let script = tmp.path().join("native_workflow_rust.py");
    fs::write(&script, adapted).expect("write adapted workflow");
    let mut actual = python_command();
    actual
        .arg(&script)
        .arg(&rust_root)
        .env("ORBIT_RESEARCH_BINARY", BINARY);
    run(actual);

    assert_eq!(
        owner_record_structure(&rust_root),
        owner_record_structure(&python_root)
    );
    for namespace in ["physics-fixture", "parallax-fixture"] {
        let expected_export = json(python_root.join(namespace).join("export.json"));
        let actual_export = json(rust_root.join(namespace).join("export.json"));
        assert_eq!(
            record_structure(&actual_export["records"]),
            record_structure(&expected_export["records"]),
            "{namespace} export"
        );
        let expected_trace = json(python_root.join(namespace).join("trace.json"));
        let actual_trace = json(rust_root.join(namespace).join("trace.json"));
        assert_eq!(
            record_structure(&actual_trace["records"]),
            record_structure(&expected_trace["records"]),
            "{namespace} trace"
        );
    }
}

#[test]
fn browser_projection_records_and_media_match_python_logically() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture_root = tmp.path().join("fixture");
    let mut fixture = python_command();
    fixture
        .arg("examples/browser_fixture.py")
        .arg(&fixture_root);
    let config_output = run(fixture);
    let config = String::from_utf8(config_output.stdout)
        .expect("config path")
        .trim()
        .to_owned();
    let python_db = tmp.path().join("python.db");
    let rust_db = tmp.path().join("rust.db");
    let python_db_text = python_db.to_str().expect("UTF-8 database");
    let rust_db_text = rust_db.to_str().expect("UTF-8 database");
    let expected_index = python_cli(&["index", "--config", &config, "--database", python_db_text]);
    let actual_index = rust(&["index", "--config", &config, "--database", rust_db_text]);
    assert_eq!(actual_index["records"], expected_index["records"]);
    assert_eq!(actual_index["pending"], expected_index["pending"]);

    let python_export = tmp.path().join("python-browser");
    let rust_export = tmp.path().join("rust-browser");
    python_cli(&[
        "browse-export",
        "--config",
        &config,
        "--database",
        python_db_text,
        "--output",
        python_export.to_str().expect("UTF-8 export"),
    ]);
    rust(&[
        "browse-export",
        "--config",
        &config,
        "--database",
        rust_db_text,
        "--output",
        rust_export.to_str().expect("UTF-8 export"),
    ]);
    let expected = json(python_export.join("index.json"));
    let actual = json(rust_export.join("index.json"));
    assert_eq!(
        projection_identity(&actual["records"]),
        projection_identity(&expected["records"])
    );
    assert_eq!(actual["media"], expected["media"]);
    assert_eq!(
        fs::read_dir(rust_export.join("media"))
            .expect("Rust media")
            .count(),
        fs::read_dir(python_export.join("media"))
            .expect("Python media")
            .count()
    );
}
