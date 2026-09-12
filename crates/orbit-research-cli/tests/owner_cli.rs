//! The owner subcommands speak the Python 0.3 argv and JSON contract.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("orbit-research-cli-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(BINARY).args(args).output().expect("cli runs")
}

fn json_stdout(output: &Output) -> Value {
    assert_eq!(
        Some(0),
        output.status.code(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON document")
}

fn write(path: &Path, document: &Value) -> String {
    std::fs::write(path, serde_json::to_vec(document).expect("bytes")).expect("request file");
    path.to_string_lossy().into_owned()
}

#[test]
fn owner_subcommands_emit_json_and_refuse_writes_inside_the_records_directory() {
    let scratch = Scratch::new();
    let root = scratch.path.join("owner");
    std::fs::create_dir_all(&root).expect("owner root");
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("code.py"), "# fixture\n").expect("fixture file");
    git(&root, &["add", "code.py"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture apparatus",
        ],
    );
    let owner_root = root.to_string_lossy().into_owned();
    let request = write(
        &scratch.path.join("program.json"),
        &json!({
            "request_id": "program-R1",
            "id": "R1",
            "scope": "synthetic-calibration",
            "expected_heads": [],
            "reason": "CLI acceptance fixture.",
            "payload": {"role": "program", "title": "CLI fixture", "question": "Does the CLI emit JSON?"},
            "orbit_links": [{"host": "fixture-host", "workspace": "ws_fixture", "task": "ORB-fixture", "run": "jrun-fixture"}],
        }),
    );
    let owner = ["--owner-root", &owner_root, "--repository", "fixture"];

    let appended = json_stdout(&run(
        &[&["program"][..], &owner, &["--request", &request]].concat()
    ));
    assert_eq!(2, appended["schema_version"]);
    let id = appended["id"].as_str().expect("id").to_owned();
    let revision = appended["revision_id"]
        .as_str()
        .expect("revision")
        .to_owned();

    // An identical request is idempotent through the CLI too.
    let repeat = json_stdout(&run(
        &[&["program"][..], &owner, &["--request", &request]].concat()
    ));
    assert_eq!(appended, repeat);

    let heads = json_stdout(&run(&[&["heads"][..], &owner, &["--id", &id]].concat()));
    assert_eq!(json!([revision]), heads["heads"]);

    git(&root, &["add", "research/records"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "immutable fixture records",
        ],
    );
    let pin = git(&root, &["rev-parse", "HEAD"]);

    let reference = json_stdout(&run(&[
        &["ref"][..],
        &owner,
        &[
            "--id",
            &id,
            "--revision",
            &revision,
            "--source-revision",
            &pin,
        ],
    ]
    .concat()));
    assert_eq!(json!("resolved"), reference["status"]);
    assert_eq!(json!(pin), reference["source_revision"]);

    let trace = json_stdout(&run(&[
        &["trace"][..],
        &owner,
        &["--id", &id, "--revision", &revision],
    ]
    .concat()));
    assert_eq!(json!("trace"), trace["kind"]);
    assert_eq!(1, trace["records"].as_array().expect("records").len());

    let destination = scratch.path.join("export.json");
    let exported = json_stdout(&run(&[
        &["export"][..],
        &owner,
        &[
            "--source-revision",
            &pin,
            "--output",
            &destination.to_string_lossy(),
        ],
    ]
    .concat()));
    assert_eq!(json!(1), exported["records"]);
    assert_eq!(json!(1), exported["manifests"]);
    let bundle: Value =
        serde_json::from_slice(&std::fs::read(&destination).expect("export")).expect("json");
    assert_eq!(json!("export"), bundle["kind"]);

    let validated = json_stdout(&run(&["validate", &destination.to_string_lossy()]));
    assert_eq!(json!(true), validated["valid"]);

    // The canonical records directory is never a destination for a generated document.
    let inside = root.join("research/records/export.json");
    let refused = run(&[
        &["export"][..],
        &owner,
        &[
            "--source-revision",
            &pin,
            "--output",
            &inside.to_string_lossy(),
        ],
    ]
    .concat());
    assert_eq!(Some(2), refused.status.code());
    assert!(
        refused.stdout.is_empty(),
        "a refusal writes nothing to stdout"
    );
    let error: Value = serde_json::from_slice(&refused.stderr).expect("stderr is JSON");
    assert_eq!(json!("invalid-input"), error["error"]["code"]);
    assert!(
        error["error"]["message"]
            .as_str()
            .expect("message")
            .contains("inside canonical records")
    );
    assert!(
        !inside.exists(),
        "nothing was written inside the records directory"
    );

    // A refused authoring request reports JSON on stderr and appends nothing.
    let stale = write(
        &scratch.path.join("stale.json"),
        &json!({
            "request_id": "program-R1-stale",
            "id": "R1",
            "scope": "synthetic-calibration",
            "expected_heads": [],
            "reason": "Stale base fixture.",
            "payload": {"role": "program", "title": "CLI fixture", "question": "Changed question?"},
            "orbit_links": [{"host": "fixture-host", "workspace": "ws_fixture", "task": "ORB-fixture", "run": "jrun-fixture"}],
        }),
    );
    let refused = run(&[&["program"][..], &owner, &["--request", &stale]].concat());
    assert_eq!(Some(2), refused.status.code());
    let error: Value = serde_json::from_slice(&refused.stderr).expect("stderr is JSON");
    assert!(
        error["error"]["message"]
            .as_str()
            .expect("message")
            .contains("stale base")
    );
}
