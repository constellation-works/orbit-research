//! The canonical plugin installed into a private HOME, exercised through real Orbit.
mod common;

use common::{Installed, Mcp, assert_cli_refusal, json_output, mcp_output};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Deref;
use std::path::Path;
use std::process::Output;

const WORKSPACE: &str = "research-v2-test";

#[test]
#[ignore = "requires ORBIT_RESEARCH_TEST_ORBIT_BIN and its native plugin sandbox"]
fn installed_plugin_serves_every_tool_over_cli_and_mcp() {
    let fixture = Fixture::new();
    fixture.install();
    let revision = fixture.git_text(&["rev-parse", "HEAD"]);
    fixture.assert_scientific_corpus_unchanged(&revision);
    orbit_research_core::application::api::Application::local(&fixture.repository)
        .expect("private corpus application")
        .link_intent("legacy-pending", "R001")
        .expect("seed an uncertain previous link without creating a task");
    fixture.restore_unprepared_legacy_journal();
    let legacy = fixture.repository.join(".git/orbit-research-operations");
    let legacy_bytes = journal_bytes(&legacy);
    assert!(
        legacy_bytes.keys().any(|name| name.ends_with(".json")),
        "the legacy journal contains an actual pending receipt"
    );
    let unprepared =
        json!({"research_id":"R001", "request_key":"unprepared", "title":"Investigate R001"});
    assert_preparation_refusal(fixture.cli_tool("link", &unprepared));
    let mut mcp = Mcp::start(&fixture, true);
    let refused = mcp.call("link", unprepared);
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(
        refused["structuredContent"]
            .to_string()
            .contains("workspace prepare-operations"),
        "{refused}"
    );
    fixture.assert_task_count(0);
    assert_eq!(
        journal_bytes(&legacy),
        legacy_bytes,
        "refusal preserves the legacy journal"
    );
    assert!(
        !legacy.join(".layout").exists(),
        "link does not prepare storage implicitly"
    );
    fixture.prepare_operations();
    assert_eq!(
        journal_bytes(&fixture.repository.join("_data/orbit-research-operations")),
        legacy_bytes,
        "preparation preserves the existing journal bytes"
    );
    fixture.assert_scientific_corpus_unchanged(&revision);
    let advertised = fixture.manifest["spec"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<BTreeSet<_>>();
    let reads = read_requests();
    let exercised = reads
        .iter()
        .map(|(tool, _)| *tool)
        .chain(["link", "validate", "accept"])
        .collect::<BTreeSet<_>>();
    assert_eq!(advertised, exercised, "exercise every advertised tool");
    for (tool, input) in &reads {
        let value = json_output(fixture.cli_tool(tool, input), tool);
        assert_read(tool, &value);
    }
    let link = json!({"research_id":"R001", "request_key":"cli-link", "title":"Investigate R001"});
    let created = json_output(fixture.cli_tool("link", &link), "link");
    assert_eq!(created["created"], true, "{created}");
    let task = created["task_id"].as_str().expect("created task id");
    let adopted = json_output(fixture.cli_tool("link", &link), "link retry");
    assert_eq!(adopted["created"], false, "{adopted}");
    assert_eq!(adopted["task_id"], task, "retry adopts the exact task");
    let validate = json!({"path": fixture.repository.to_string_lossy()});
    assert_cli_refusal(
        fixture.cli_tool("validate", &validate),
        "run_context_required",
    );
    let accept = json!({"task_id":task, "research_id":"R001"});
    assert_cli_refusal(fixture.cli_tool("accept", &accept), "refused");

    let listed = mcp.request("tools/list", json!({}));
    let names = listed["tools"]
        .as_array()
        .expect("MCP tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter_map(|name| name.strip_prefix("research_"))
        .collect::<BTreeSet<_>>();
    assert_eq!(names, advertised, "the MCP surface lists every plugin tool");
    for (tool, input) in reads {
        let result = mcp.call(tool, input);
        assert_ne!(result["isError"], true, "{tool}: {result}");
        assert_read(tool, &mcp_output(&result));
    }
    let link = json!({"research_id":"R001", "request_key":"mcp-link", "title":"Investigate R001"});
    let created = mcp.call("link", link.clone());
    assert_ne!(created["isError"], true, "{created}");
    assert_eq!(created["structuredContent"]["created"], true, "{created}");
    let adopted = mcp.call("link", link);
    assert_ne!(adopted["isError"], true, "{adopted}");
    assert_eq!(adopted["structuredContent"]["created"], false, "{adopted}");
    assert_eq!(
        adopted["structuredContent"]["task_id"],
        created["structuredContent"]["task_id"]
    );
    for (tool, input, code) in [
        ("validate", validate, "run_context_required"),
        ("accept", accept, "refused"),
    ] {
        let result = mcp.call(tool, input);
        assert_eq!(result["isError"], true, "{tool}: {result}");
        assert_eq!(result["structuredContent"]["code"], code, "{result}");
    }
    let invalid = mcp.call("version", json!({"unknown_field":true}));
    assert_eq!(invalid["isError"], true, "{invalid}");
    assert!(
        invalid["structuredContent"]
            .to_string()
            .contains("unknown_field"),
        "{invalid}"
    );
    fixture.assert_task_count(2);
    fixture.assert_scientific_corpus_unchanged(&revision);
}

/// A panel source runs as a read-only tool for a caller that is not an
/// operator, and `awaiting-acceptance` reads each delivered result's task
/// through the `orbit.task.show` and `orbit.task.artifact.get` callbacks. This
/// proves those callbacks answer for such a caller (an MCP session without
/// `--operator`) and for the operator CLI alike: a delivered result whose task
/// has no acceptance artifact is listed as awaiting, never as unknown.
#[test]
#[ignore = "requires ORBIT_RESEARCH_TEST_ORBIT_BIN and its native plugin sandbox"]
fn awaiting_acceptance_reads_task_artifacts_through_callbacks() {
    let fixture = Fixture::new();
    fixture.install();
    let link =
        json!({"research_id":"R001", "request_key":"panel-link", "title":"Investigate R001"});
    let created = json_output(fixture.cli_tool("link", &link), "link");
    let task = created["task_id"].as_str().expect("created task id");
    fixture.deliver_r001(task);
    let expected = json!([{
        "id": "R001",
        "result": "Study",
        "status": "awaiting acceptance",
        "task": task,
        "updated": created_date(&fixture),
    }]);
    let value = json_output(
        fixture.cli_tool("awaiting-acceptance", &json!({})),
        "awaiting-acceptance over CLI",
    );
    assert_eq!(value, expected, "operator CLI");
    let mut mcp = Mcp::start(&fixture, false);
    let result = mcp.call("awaiting-acceptance", json!({}));
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(mcp_output(&result), expected, "agent MCP session");
}

fn created_date(fixture: &Fixture) -> String {
    let text = fs::read_to_string(fixture.repository.join("research/R001-study/README.md"))
        .expect("delivered R001");
    text.lines()
        .find_map(|line| line.strip_prefix("updated: "))
        .expect("updated date")
        .trim_matches(['\'', '"'])
        .to_owned()
}

fn read_requests() -> Vec<(&'static str, Value)> {
    vec![
        ("version", json!({})),
        ("list", json!({})),
        ("show", json!({"id":"R001"})),
        ("check", json!({})),
        (
            "plan",
            json!({"shape":"investigation", "research_id":"R001", "objective":"Reproduce the baseline"}),
        ),
        ("open-questions", json!({})),
        ("awaiting-acceptance", json!({})),
        ("hypotheses", json!({})),
        ("corpus-health", json!({})),
    ]
}

fn assert_read(tool: &str, value: &Value) {
    match tool {
        "version" => assert_eq!(
            value["core_version"],
            orbit_research_core::VERSION,
            "{value}"
        ),
        "list" => assert_eq!(value["records"][0]["id"], "R001", "{value}"),
        "show" => assert_eq!(value["id"], "R001", "{value}"),
        "check" => {
            assert_eq!(value["valid"], true, "{value}");
            assert_eq!(value["record_count"], 1, "{value}");
        }
        "plan" => assert_eq!(
            value["context_files"],
            json!(["dir:research/R001-study"]),
            "{value}"
        ),
        // The fixture holds one reserved R: no question, hypothesis or delivered
        // result, so each panel answers with its readable empty state.
        "open-questions" => assert_status_row(value, "No questions captured yet"),
        "awaiting-acceptance" => assert_status_row(value, "No delivered results yet"),
        "hypotheses" => assert_status_row(value, "No hypotheses yet"),
        "corpus-health" => {
            assert_eq!(value["Corpus"], "Valid", "{value}");
            assert_eq!(value["Records"], 1, "{value}");
        }
        _ => panic!("unexpected read tool: {tool}"),
    }
}

fn assert_status_row(value: &Value, expected: &str) {
    let rows = value.as_array().expect("table panel output");
    assert_eq!(rows.len(), 1, "{value}");
    assert!(
        rows[0]["status"]
            .as_str()
            .is_some_and(|status| status.starts_with(expected)),
        "{value}"
    );
}

fn assert_preparation_refusal(output: Output) {
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let value: Value = serde_json::from_str(stderr.lines().last().expect("error JSON line"))
        .expect("error object");
    assert!(
        value.to_string().contains("workspace prepare-operations"),
        "{value}"
    );
}

fn journal_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .expect("private operations journal")
        .filter_map(|entry| {
            let entry = entry.expect("journal entry");
            assert!(entry.file_type().expect("entry type").is_file());
            // Preparation adds its versioned ledger; existing receipt and
            // lock bytes must remain identical across that explicit change.
            if entry.file_name() == ".layout" {
                return None;
            }
            Some((
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).expect("journal bytes"),
            ))
        })
        .collect()
}

/// The shared installed-plugin fixture plus the one reserved research record
/// this test's scenarios start from.
struct Fixture {
    installed: Installed,
}

impl Deref for Fixture {
    type Target = Installed;

    fn deref(&self) -> &Installed {
        &self.installed
    }
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            installed: Installed::new(WORKSPACE),
        };
        let output = fixture
            .research_command()
            .args(["research", "create", "--corpus"])
            .arg(&fixture.repository)
            .args([
                "--kind",
                "R",
                "--status",
                "planned",
                "--title",
                "Study",
                "--body",
                "Question under study",
                "--request-key",
                "reserve",
                "--json",
            ])
            .output()
            .expect("reserve a research record");
        assert!(output.status.success(), "{output:?}");
        fixture
    }

    fn assert_scientific_corpus_unchanged(&self, revision: &str) {
        assert_eq!(self.git_text(&["rev-parse", "HEAD"]), revision);
        assert_eq!(
            self.git_text(&["status", "--porcelain", "--untracked-files=all"]),
            "",
            "operational linking keeps the scientific corpus clean"
        );
    }

    fn restore_unprepared_legacy_journal(&self) {
        let legacy = self.repository.join(".git/orbit-research-operations");
        let prepared = self.repository.join("_data/orbit-research-operations");
        assert!(
            legacy.is_file(),
            "fresh initialization leaves an old-client marker"
        );
        assert!(
            prepared.is_dir(),
            "fresh initialization prepares operations"
        );
        // Only this disposable corpus is reversed, before any task links exist.
        fs::remove_file(&legacy).expect("remove the private fresh marker");
        fs::remove_file(prepared.join(".layout")).expect("remove the private fresh ledger");
        fs::rename(prepared, legacy).expect("model the previous journal layout");
    }

    fn prepare_operations(&self) {
        let output = self
            .research_command()
            .args(["workspace", "prepare-operations"])
            .arg(&self.repository)
            .arg("--json")
            .output()
            .expect("prepare the private existing corpus");
        assert!(output.status.success(), "{output:?}");
    }

    /// Commit R001 the way a finished run leaves it: done, with the run's
    /// task and run recorded in its frontmatter.
    fn deliver_r001(&self, task: &str) {
        let path = self.repository.join("research/R001-study/README.md");
        let text = fs::read_to_string(&path).expect("reserved R001");
        let (front, body) = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .expect("R001 frontmatter");
        let front = front.replace("status: planned", "status: done");
        fs::write(
            &path,
            format!("---\n{front}\norbit:\n  task: {task}\n  run: jrun-fixture\n---\n{body}"),
        )
        .expect("deliver R001");
        for args in [
            vec!["add", "--", "research"],
            vec!["commit", "-m", "Deliver R001 as a finished run would"],
        ] {
            let output = self
                .command_for(Path::new("git"))
                .args(&args)
                .output()
                .expect("commit the delivered fixture result");
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
    }

    fn assert_task_count(&self, expected: usize) {
        let output = self
            .command()
            .env("ORBIT_OPERATOR", "1")
            .args(["task", "list", "--workspace", WORKSPACE, "--json"])
            .output()
            .expect("observe private tasks through Orbit");
        let tasks = json_output(output, "private task list");
        assert_eq!(
            tasks.as_array().expect("task records").len(),
            expected,
            "{tasks}"
        );
    }
}
