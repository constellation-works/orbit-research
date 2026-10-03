//! `assess` through the real binary with production composition: the
//! acceptance lookup spawns `orbit` (here a fake executable named by
//! `ORBIT_BIN` that returns canned task JSON) from the corpus checkout. No real
//! Orbit, plugin installation or grant is involved.
#![cfg(unix)]

mod exec_support;

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

/// One shared script. Its canned outputs live in the corpus's `.git/fake-orbit`
/// (invisible to `git status`), found from the working directory the writer runs
/// it in, so each test keeps its own state without rewriting the executable.
fn fake_orbit() -> &'static Path {
    static SCRIPT: OnceLock<PathBuf> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        let path = tempfile::tempdir().expect("script dir").keep().join("orbit");
        exec_support::install_executable(
            &path,
            "#!/bin/sh\nd=\"$PWD/.git/fake-orbit\"\nprintf '%s\\n' \"$3 $5\" >> \"$d/calls\"\nif [ -f \"$d/$3.fail\" ]; then cat \"$d/$3.fail\" >&2; exit 1; fi\ncat \"$d/$3.out\"\n",
        );
        path
    })
}

fn command(orbit: &Path) -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("ORBIT_BIN", orbit)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Assess fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Assess fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Assess fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Assess fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("run fixture Git");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("Git text")
        .trim()
        .to_owned()
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    readme: String,
}

impl Fixture {
    /// A corpus with hypothesis H001 and a delivered R001 linked to `task-1`.
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary corpus");
        let root = temp.path().join("corpus");
        let run = |args: &[&str]| {
            let output = command(fake_orbit())
                .args(args)
                .output()
                .expect("run writer");
            assert!(output.status.success(), "{args:?}: {output:?}");
        };
        run(&["workspace", "init", root.to_str().expect("UTF-8 path")]);
        for (kind, extra) in [("H", None), ("R", Some("planned"))] {
            let mut args = vec![
                "research",
                "create",
                "--corpus",
                root.to_str().expect("UTF-8 path"),
                "--kind",
                kind,
                "--title",
                "Study",
                "--body",
                "Body",
                "--request-key",
                kind,
            ];
            if let Some(status) = extra {
                args.extend(["--status", status]);
            }
            run(&args);
        }
        // Primary-mode `revise` refuses R: a run delivers it. Model the delivery by
        // linking the reserved stub to its task, as the run's own commit would.
        let readme = git(&root, &["ls-files", "research/*/README.md"]);
        let path = root.join(&readme);
        let text = fs::read_to_string(&path).expect("R README");
        let (head, rest) = text.split_once("\n---\n").expect("frontmatter");
        fs::write(
            &path,
            format!("{head}\norbit: {{task: task-1, run: run-1}}\n---\n{rest}"),
        )
        .expect("link R to its task");
        git(&root, &["commit", "-qam", "Deliver R001"]);
        fs::create_dir_all(root.join(".git/fake-orbit")).expect("fake state");
        Self {
            _temp: temp,
            root,
            readme,
        }
    }

    fn state(&self, name: &str, contents: &str) {
        let path = self.root.join(".git/fake-orbit");
        let _ = fs::remove_file(path.join(name.replace(".out", ".fail")));
        fs::write(path.join(name), contents).expect("canned output");
    }

    fn blob(&self) -> String {
        git(&self.root, &["rev-parse", &format!("HEAD:{}", self.readme)])
    }

    fn accepted(&self, research: &str, blob: &str) {
        self.state(
            "orbit.task.show.out",
            &json!({"artifacts": [{"path": "research-acceptance.json"}]}).to_string(),
        );
        let artifact = json!({
            "research_id": research, "commit": git(&self.root, &["rev-parse", "HEAD"]),
            "blob": blob, "run_id": "run-1", "artifact_digests": {},
        });
        self.state(
            "orbit.task.artifact.get.out",
            &json!({"media_type": "application/json", "content": artifact.to_string()}).to_string(),
        );
    }

    fn hypothesis_blob(&self) -> String {
        git(&self.root, &["rev-parse", "HEAD:hypotheses/H001-study.md"])
    }

    fn assess_args(&self) -> Vec<String> {
        [
            "--json",
            "research",
            "assess",
            "--corpus",
            self.root.to_str().expect("UTF-8 path"),
            "--id",
            "H001",
            "--expected-blob",
            &self.hypothesis_blob(),
            "--research",
            "R001",
            "--revision",
            "1",
            "--verdict",
            "inconclusive",
            "--strength",
            "anecdote",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn assess(&self, orbit: &Path) -> Output {
        command(orbit)
            .args(self.assess_args())
            .output()
            .expect("assess through CLI")
    }

    /// Run `assess` and require a refusal that leaves HEAD and the tree untouched.
    fn refused(&self, orbit: &Path) -> String {
        let head = git(&self.root, &["rev-parse", "HEAD"]);
        let output = self.assess(orbit);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let error: Value = serde_json::from_slice(&output.stderr).expect("CLI error JSON");
        assert_eq!(git(&self.root, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(&self.root, &["status", "--porcelain"]), "");
        error["error"]["message"]
            .as_str()
            .expect("diagnostic")
            .to_owned()
    }
}

#[test]
fn assess_refuses_each_unverifiable_acceptance_through_the_real_cli() {
    let fixture = Fixture::new();
    let orbit = fake_orbit();

    // No artifact stored on the task.
    fixture.state("orbit.task.show.out", &json!({"artifacts": []}).to_string());
    let message = fixture.refused(orbit);
    assert!(
        message.contains("no `research-acceptance.json`") && message.contains("task-1"),
        "{message}"
    );

    // The artifact accepts a different record.
    fixture.accepted("R002", &fixture.blob());
    let message = fixture.refused(orbit);
    assert!(message.contains("accepts R002, not R001"), "{message}");

    // The README changed after acceptance.
    fixture.accepted("R001", &"1".repeat(40));
    let message = fixture.refused(orbit);
    assert!(
        message.contains("changed after task task-1 accepted it"),
        "{message}"
    );

    // Orbit fails.
    fixture.state("orbit.task.show.fail", "workspace not registered");
    let message = fixture.refused(orbit);
    assert!(
        message.contains("workspace not registered") && message.contains("ORBIT_BIN"),
        "{message}"
    );

    // Orbit cannot be started at all.
    let message = fixture.refused(&fixture.root.join("no-such-orbit"));
    assert!(
        message.contains("failed to invoke") && message.contains("ORBIT_BIN"),
        "{message}"
    );

    // The artifact is not the accept shape.
    fixture.state(
        "orbit.task.show.out",
        &json!({"artifacts": [{"path": "research-acceptance.json"}]}).to_string(),
    );
    fixture.state(
        "orbit.task.artifact.get.out",
        &json!({"content": "{}"}).to_string(),
    );
    let message = fixture.refused(orbit);
    assert!(message.contains("unreadable"), "{message}");
}

#[test]
fn assess_appends_the_verdict_when_the_task_artifact_matches_the_current_readme() {
    let fixture = Fixture::new();
    fixture.accepted("R001", &fixture.blob());
    let output = fixture.assess(fake_orbit());
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).expect("assess JSON");
    assert_eq!(result["mode"], "primary", "{result}");
    let calls = fs::read_to_string(fixture.root.join(".git/fake-orbit/calls")).expect("calls");
    assert!(
        calls.contains("orbit.task.artifact.get {\"id\":\"task-1\""),
        "{calls}"
    );
    let hypothesis =
        fs::read_to_string(fixture.root.join("hypotheses/H001-study.md")).expect("H001");
    assert!(hypothesis.contains("verdict: inconclusive"), "{hypothesis}");
    assert_eq!(git(&fixture.root, &["status", "--porcelain"]), "");

    // The same lookup serves the MCP transport.
    let before = git(&fixture.root, &["rev-parse", "HEAD"]);
    let mut child = command(fake_orbit())
        .args(["mcp", "--corpus"])
        .arg(&fixture.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start MCP");
    let mut input = child.stdin.take().expect("MCP stdin");
    let assess = json!({"name":"research.assess", "arguments":{
        "id":"H001", "expected_blob": fixture.hypothesis_blob(), "research":"R001",
        "revision":1, "verdict":"refutes", "strength":"suggestive"}});
    for value in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":assess}),
    ] {
        writeln!(input, "{value}").expect("write MCP request");
    }
    drop(input);
    let output = child.wait_with_output().expect("finish MCP");
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect();
    assert_eq!(responses[1]["result"]["isError"], false, "{responses:?}");
    assert_ne!(git(&fixture.root, &["rev-parse", "HEAD"]), before);
}
