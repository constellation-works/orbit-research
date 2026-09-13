//! The `index`, `index-trace` and `browse-export` subcommands speak the JSON/exit-code
//! contract in `specs/cli-compat.md`: one JSON document on stdout on success, exit 0;
//! `{"error":{"code":"invalid-input",...}}` on stderr, exit 2, for a refused request.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::Digest as _;

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
        let path = std::env::temp_dir().join(format!("orbit-research-index-cli-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().expect("git runs");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
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

fn json_stderr(output: &Output) -> Value {
    assert_eq!(Some(2), output.status.code(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    serde_json::from_slice(&output.stderr).expect("stderr is one JSON document")
}

fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, serde_json::to_vec(value).expect("bytes")).expect("write");
}

/// One committed, resolved program record plus the manifest that declares it: the smallest
/// fixture that `index` accepts.
fn build_fixture(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let checkout = scratch.join("owner");
    std::fs::create_dir_all(&checkout).expect("checkout root");
    git(&checkout, &["init", "-q"]);
    std::fs::write(checkout.join(".keep"), b"Fixture checkout; no scientific record.\n").expect("seed file");
    git(&checkout, &["add", "."]);
    git(
        &checkout,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture checkout",
        ],
    );

    let placeholder_sha256 = format!("sha256:{}", "0".repeat(64));
    let mut record = json!({
        "schema_version": 1,
        "kind": "program",
        "id": "urn:research:fixture:program:P1",
        "aliases": [],
        "activity": "active",
        "scope": "synthetic-calibration",
        "provenance": {
            "repository": "fixture",
            "git_revision": Value::Null,
            "blob_oid": Value::Null,
            "sha256": placeholder_sha256,
            "path": "research/records/P1.json",
            "selector": "$",
            "historical": false,
            "working_tree": true,
        },
        "limitations": [],
        "missingness": [],
        "legacy": Value::Null,
        "references": [],
        "presentation": {},
        "payload": {"role": "program", "title": "CLI fixture"},
    });
    record.as_object_mut().expect("object").remove("revision_id");
    let digest = orbit_research_contract::revision_digest(&record).expect("digest");
    record["revision_id"] = json!(digest);

    let relative = "research/records/P1.json";
    let bytes = orbit_research_contract::canonical_json(&record).expect("canonical json");
    let full = checkout.join(relative);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("record directory");
    std::fs::write(&full, &bytes).expect("write record");
    git(&checkout, &["add", "research"]);
    git(
        &checkout,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "immutable fixture record",
        ],
    );
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    let sha256 = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
    record["provenance"] = json!({
        "repository": "fixture",
        "git_revision": head,
        "blob_oid": Value::Null,
        "sha256": sha256,
        "path": relative,
        "selector": "$",
        "historical": false,
        "working_tree": false,
    });

    let documents = scratch.join("documents");
    write_json(&documents.join("program.json"), &record);
    write_json(
        &documents.join("manifest.json"),
        &json!({
            "schema_version": 1,
            "kind": "manifest",
            "repositories": [{"id": "fixture", "git_revision": head}],
            "references": [{
                "repository": "fixture",
                "id": record["id"],
                "revision_id": record["revision_id"],
                "source_revision": head,
                "status": "resolved",
            }],
        }),
    );

    let config = json!({
        "schema_version": 1,
        "checkouts": {"fixture": checkout.to_string_lossy()},
        "documents": [documents.to_string_lossy()],
    });
    let config_path = scratch.join("index-config.json");
    write_json(&config_path, &config);
    (checkout, config_path)
}

#[test]
fn index_and_index_trace_match_cli_compat_flags_and_json() {
    let scratch = Scratch::new();
    let (_checkout, config_path) = build_fixture(&scratch);
    let database = scratch.join("index.sqlite");

    let output = run(&[
        "index",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--database",
        database.to_str().expect("utf8 path"),
    ]);
    let body = json_stdout(&output);
    assert_eq!(1, body["records"], "{body}");
    assert_eq!(0, body["pending"], "{body}");
    assert!(body["content_digest"].as_str().is_some_and(|digest| digest.starts_with("sha256:")), "{body}");
    assert_eq!(database.to_string_lossy(), body["database"].as_str().unwrap_or_default());

    let projection = orbit_research_index::read_index(&database).expect("read index directly");
    let key = projection["records"][0]["key"].as_str().expect("key").to_owned();

    let trace_output = run(&["index-trace", "--database", database.to_str().expect("utf8 path"), "--key", &key]);
    let trace = json_stdout(&trace_output);
    assert_eq!(key, trace["root"]);
    assert_eq!(1, trace["records"].as_array().expect("records").len());

    // An exact but absent key is a refused request, not a crash or an empty success.
    let missing_key = "0".repeat(64);
    let missing = run(&["index-trace", "--database", database.to_str().expect("utf8 path"), "--key", &missing_key]);
    let error = json_stderr(&missing);
    assert_eq!("invalid-input", error["error"]["code"]);
}

#[test]
fn index_refuses_an_invalid_document_with_problems_on_stderr() {
    let scratch = Scratch::new();
    let (_checkout, config_path) = build_fixture(&scratch);
    let documents = scratch.join("documents");
    std::fs::write(documents.join("program.json"), b"{not json").expect("corrupt document");
    let database = scratch.join("index.sqlite");

    let output = run(&[
        "index",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--database",
        database.to_str().expect("utf8 path"),
    ]);
    let error = json_stderr(&output);
    assert_eq!("invalid-input", error["error"]["code"]);
    assert!(
        error["error"]["problems"].as_array().is_some_and(|problems| !problems.is_empty()),
        "{error}"
    );
    assert!(!database.exists(), "an invalid first rebuild must not create a database");
}

#[test]
fn browse_export_refuses_an_existing_destination_and_a_destination_inside_an_owner_checkout() {
    let scratch = Scratch::new();
    let (checkout, config_path) = build_fixture(&scratch);
    let database = scratch.join("index.sqlite");
    assert_eq!(
        Some(0),
        run(&[
            "index",
            "--config",
            config_path.to_str().expect("utf8 path"),
            "--database",
            database.to_str().expect("utf8 path"),
        ])
        .status
        .code()
    );

    let site = scratch.join("site");
    let first = run(&[
        "browse-export",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--database",
        database.to_str().expect("utf8 path"),
        "--output",
        site.to_str().expect("utf8 path"),
    ]);
    let body = json_stdout(&first);
    assert_eq!(1, body["records"], "{body}");
    assert!(site.join("index.html").is_file());
    assert!(site.join("app.js").is_file());
    assert!(site.join("style.css").is_file());
    assert!(site.join("index.json").is_file());
    assert!(site.join("data.js").is_file());

    // The destination must be new; retain the previous usable export.
    let second = run(&[
        "browse-export",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--database",
        database.to_str().expect("utf8 path"),
        "--output",
        site.to_str().expect("utf8 path"),
    ]);
    let error = json_stderr(&second);
    assert_eq!("invalid-input", error["error"]["code"]);
    assert!(site.join("index.html").is_file(), "the previous export must survive the refusal");

    // Writing inside a mapped owner checkout is forbidden, even for a brand-new path.
    let inside = checkout.join("site");
    let third = run(&[
        "browse-export",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--database",
        database.to_str().expect("utf8 path"),
        "--output",
        inside.to_str().expect("utf8 path"),
    ]);
    let error = json_stderr(&third);
    assert_eq!("invalid-input", error["error"]["code"]);
    assert!(!inside.exists());
}
