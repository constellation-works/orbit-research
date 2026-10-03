use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "MCP workflow fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "MCP workflow fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn request(root: &Path, method: &str, params: Value) -> Value {
    let mut child = command()
        .args(["mcp", "--corpus"])
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start MCP server");
    let mut input = child.stdin.take().expect("MCP stdin");
    for value in [
        json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{}}),
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0", "id":"operation", "method":method, "params":params}),
    ] {
        serde_json::to_writer(&mut input, &value).expect("write MCP request");
        writeln!(input).expect("terminate MCP request");
    }
    drop(input);
    let output = child.wait_with_output().expect("finish MCP server");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect();
    assert_eq!(responses.len(), 2, "{responses:?}");
    assert_eq!(responses[1]["id"], "operation");
    assert!(responses[1].get("error").is_none(), "{responses:?}");
    responses[1]["result"].clone()
}

fn tool(root: &Path, name: &str, arguments: Value) -> Value {
    let result = request(
        root,
        "tools/call",
        json!({"name":name, "arguments":arguments}),
    );
    assert_eq!(result["isError"], false, "{name}: {result}");
    serde_json::from_str(result["content"][0]["text"].as_str().expect("result text"))
        .expect("application JSON")
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run fixture Git");
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

#[test]
fn every_advertised_research_tool_runs_through_the_real_mcp_transport() {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path().join("corpus");
    let initialized = command()
        .args(["workspace", "init"])
        .arg(&root)
        .output()
        .expect("initialize corpus");
    assert!(initialized.status.success(), "{initialized:?}");

    let definitions = request(&root, "tools/list", json!({}));
    let names: BTreeSet<_> = definitions["tools"]
        .as_array()
        .expect("tool definitions")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "research.work_links",
            "research.list",
            "research.show",
            "research.check",
            "research.create",
            "research.capture",
            "research.revise",
            "research.assess",
            "research.revise_question",
            "research.plan",
        ])
    );

    let create = json!({"kind":"Q", "title":"Transport question", "body":"Original body", "request_key":"mcp-question"});
    let question = tool(&root, "research.create", create.clone());
    assert_eq!(question["id"], "Q001");
    let committed = git(&root, &["rev-parse", "HEAD"]);
    let mut replay = tool(&root, "research.create", create);
    assert_eq!(replay["replayed"], true);
    replay.as_object_mut().expect("receipt").remove("replayed");
    assert_eq!(replay, question);
    assert!(question.get("replayed").is_none());
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), committed);

    let captured = tool(
        &root,
        "research.capture",
        json!({"text":"Captured question", "tags":["inbox"]}),
    );
    assert_eq!(captured["id"], "Q002");
    let before = tool(&root, "research.show", json!({"id":"Q001"}));
    let revision = json!({"id":"Q001", "expected_blob":before["git_blob"], "title":"Revised question", "body":"Revised body", "tags":["inbox"]});
    let revised = tool(&root, "research.revise", revision.clone());
    assert_eq!(revised["mode"], "primary");
    let mut retried = tool(&root, "research.revise", revision);
    assert_eq!(retried["replayed"], true);
    retried.as_object_mut().expect("receipt").remove("replayed");
    assert_eq!(retried, revised);
    let after = tool(&root, "research.show", json!({"id":"Q001"}));
    assert_eq!(after["metadata"]["title"], "Revised question");
    assert!(
        after["body"]
            .as_str()
            .expect("record body")
            .contains("Revised body")
    );

    let capture = tool(&root, "research.show", json!({"id":"Q002"}));
    tool(
        &root,
        "research.revise_question",
        json!({"id":"Q002", "expected_blob":capture["git_blob"], "title":"Revised capture", "body":"New body", "tags":[]}),
    );
    assert_eq!(
        tool(&root, "research.show", json!({"id":"Q002"}))["metadata"]["title"],
        "Revised capture"
    );

    for (kind, title, key) in [
        ("H", "Hypothesis", "mcp-hypothesis"),
        ("R", "Reserved research", "mcp-research"),
    ] {
        tool(
            &root,
            "research.create",
            json!({"kind":kind, "title":title, "request_key":key}),
        );
    }
    {
        // A synthesis plan needs its contribution committed.
        let findings = root.join("research/R001-reserved-research/artifacts/work");
        std::fs::create_dir_all(&findings).expect("contribution directory");
        std::fs::write(findings.join("findings.md"), "Findings.\n").expect("findings");
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "-m",
                "contribution",
            ],
        );
    }
    let head = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(
        tool(&root, "research.list", json!({}))["records"]
            .as_array()
            .expect("records")
            .len(),
        4
    );
    assert_eq!(tool(&root, "research.check", json!({}))["record_count"], 4);
    assert_eq!(tool(&root, "research.work_links", json!({})), json!([]));
    for plan in [
        json!({"shape":"investigation", "research_id":"R001", "objective":"Test transport"}),
        json!({"shape":"contribution", "research_id":"R001", "objective":"Test transport", "unit":"work"}),
        json!({"shape":"synthesis", "research_id":"R001", "units":["work"]}),
    ] {
        assert!(
            tool(&root, "research.plan", plan)["title"]
                .as_str()
                .expect("draft title")
                .contains("R001")
        );
    }
    let stale = request(
        &root,
        "tools/call",
        json!({"name":"research.revise", "arguments":{"id":"Q001", "expected_blob":before["git_blob"], "title":"Stale edit"}}),
    );
    assert_eq!(stale["isError"], true, "{stale}");
    assert!(
        stale["content"][0]["text"]
            .as_str()
            .expect("error text")
            .contains("run `research show --id Q001` again and use its `git_blob`"),
        "{stale}"
    );
    let hypothesis = tool(&root, "research.show", json!({"id":"H001"}));
    let assessment = request(
        &root,
        "tools/call",
        json!({"name":"research.assess", "arguments":{"id":"H001", "expected_blob":hypothesis["git_blob"], "research":"R001", "revision":1, "verdict":"inconclusive", "strength":"anecdote"}}),
    );
    assert_eq!(assessment["isError"], true, "{assessment}");
    assert!(
        assessment["content"][0]["text"]
            .as_str()
            .expect("error text")
            .contains("no `orbit.task`")
    );
    let cli_assessment = command()
        .args(["--json", "research", "assess", "--corpus"])
        .arg(&root)
        .args([
            "--id",
            "H001",
            "--expected-blob",
            hypothesis["git_blob"].as_str().expect("hypothesis blob"),
            "--research",
            "R001",
            "--revision",
            "1",
            "--verdict",
            "inconclusive",
            "--strength",
            "anecdote",
        ])
        .output()
        .expect("assess through CLI");
    assert_eq!(cli_assessment.status.code(), Some(1), "{cli_assessment:?}");
    assert!(cli_assessment.stdout.is_empty(), "{cli_assessment:?}");
    let error: Value = serde_json::from_slice(&cli_assessment.stderr).expect("CLI error JSON");
    assert!(
        error["error"]["message"]
            .as_str()
            .expect("CLI diagnostic")
            .contains("no `orbit.task`")
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
}
