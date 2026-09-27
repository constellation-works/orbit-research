//! `orbit plugin test`'s conformance workspace is always an empty, non-Git
//! temporary directory with no seeding mechanism (verified against Orbit's
//! own `orbit-core/src/application/plugin/conformance.rs`), so it can only
//! exercise this backend's environment-independent refusals. These tests
//! cover what that sandbox cannot: real success output and the "unknown
//! record id" refusal against an actual git-backed corpus, by calling the
//! same `serve_plugin_tool_call` entry point the `orbit-tool` subcommand
//! serves stdin/stdout through.
use super::super::plugin::serve_plugin_tool_call;
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("../../../orbit-research-core/tests/fixtures/schema.json");

fn corpus() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).expect("schema directory");
    fs::write(root.join("_scripts/schema.json"), SCHEMA).expect("fixture schema");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("record directory");
    }
    fs::write(
        root.join("questions/Q001-why.md"),
        "---\nid: Q001\ntitle: Why\nstatus: answered\ntags: [logic, evidence]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-02\nanswered_by: []\n---\nQuestion body.",
    )
    .expect("fixture record");
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "plugin-tests@example.invalid"]);
    run(&["config", "user.name", "Plugin tests"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "fixture"]);
    temp
}

fn call(request: &Value) -> Value {
    let mut output = Vec::new();
    serve_plugin_tool_call(Cursor::new(request.to_string().into_bytes()), &mut output)
        .expect("serve one plugin tool call");
    serde_json::from_slice(&output).expect("one JSON reply")
}

fn envelope(tool: &str, input: Value, workspace_root: Option<&Path>) -> Value {
    let mut request = json!({"schema_version": 1, "tool": tool, "input": input});
    if let Some(root) = workspace_root {
        request["context"] = json!({"workspace_root": root.to_string_lossy()});
    }
    request
}

#[test]
fn version_succeeds_with_no_bound_workspace() {
    let reply = call(&envelope("version", json!({}), None));
    assert_eq!(reply["ok"], true);
    assert_eq!(
        reply["output"]["core_version"],
        orbit_research_core::VERSION
    );
}

#[test]
fn list_reads_the_fixture_corpus() {
    let temp = corpus();
    let reply = call(&envelope("research.list", json!({}), Some(temp.path())));
    assert_eq!(reply["ok"], true);
    let records = reply["output"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "Q001");
}

#[test]
fn check_reports_the_fixture_summary() {
    let temp = corpus();
    let reply = call(&envelope(
        "orbit.research.check",
        json!({}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["output"]["valid"], true);
    assert_eq!(reply["output"]["record_count"], 1);
}

#[test]
fn show_returns_the_matching_record() {
    let temp = corpus();
    let reply = call(&envelope("show", json!({"id": "Q001"}), Some(temp.path())));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["output"]["id"], "Q001");
    assert_eq!(reply["output"]["body"], "Question body.");
}

#[test]
fn show_refuses_an_unknown_id_with_a_typed_error() {
    let temp = corpus();
    let reply = call(&envelope("show", json!({"id": "Q999"}), Some(temp.path())));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "record_not_found");
    assert_eq!(reply["error"]["retryable"], false);
    assert!(
        reply["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("Q999")
    );
}

#[test]
fn show_refuses_a_malformed_id_as_an_invalid_request() {
    let temp = corpus();
    let reply = call(&envelope(
        "show",
        json!({"id": "nonsense"}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "invalid_request");
}

#[test]
fn unknown_tool_is_refused_as_an_invalid_request() {
    let temp = corpus();
    let reply = call(&envelope(
        "research.frobnicate",
        json!({}),
        Some(temp.path()),
    ));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "invalid_request");
}

#[test]
fn a_workspace_scoped_tool_without_a_bound_workspace_is_refused() {
    let reply = call(&envelope("list", json!({}), None));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "workspace_required");
}

#[test]
fn an_unopenable_workspace_is_a_typed_corpus_refusal_not_a_crash() {
    let empty = tempfile::tempdir().expect("empty directory");
    let reply = call(&envelope("check", json!({}), Some(empty.path())));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "corpus_unavailable");
}
