//! `assess` against a real Orbit installation in a private HOME and Orbit
//! root. It installs and enables no plugin: the `research-acceptance.json`
//! artifact the plugin's `accept` tool would store is put through Orbit's own
//! artifact tool, and the writer then fetches it through the real `orbit`
//! binary. Plugin-side `accept` is covered by `plugin_v2.rs` and the
//! deterministic goldens in `src/tests/plugin.rs`; both share the `Acceptance`
//! type this reads.
#![cfg(unix)]

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");
const WORKSPACE: &str = "assess-installed-test";

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    orbit: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let orbit = std::env::var_os("ORBIT_RESEARCH_TEST_ORBIT_BIN")
            .filter(|value| !value.is_empty())
            .expect("set ORBIT_RESEARCH_TEST_ORBIT_BIN to run installed-Orbit QA");
        let orbit = Path::new(&orbit)
            .canonicalize()
            .expect("Orbit executable path");
        let temp = tempfile::tempdir().expect("private fixture");
        let base = temp.path().canonicalize().expect("physical fixture root");
        let fixture = Self {
            root: base.join("corpus"),
            home: base.join("home"),
            orbit,
            _temp: temp,
        };
        fs::create_dir(&fixture.home).expect("private HOME");
        let root = fixture.root.to_str().expect("UTF-8 path").to_owned();
        // `workspace init` creates the corpus directory, so run it from the base.
        let mut init = fixture.command(Path::new(BINARY));
        init.current_dir(&base).args(["workspace", "init", &root]);
        fixture.ok(&mut init);
        fixture.ok(fixture.orbit_command().args([
            "workspace",
            "init",
            "--name",
            WORKSPACE,
            "--ship-mode",
            "local",
        ]));
        // Orbit initialization adds its managed ignore block; commit it so the
        // writer sees a clean checkout.
        fixture.git(&["add", "--", ".gitignore"]);
        fixture.git(&["commit", "-qm", "Finalize isolated Orbit fixture ignore"]);
        for (kind, status) in [("H", None), ("R", Some("planned"))] {
            let mut create = fixture.command(Path::new(BINARY));
            create.args([
                "research",
                "create",
                "--corpus",
                &root,
                "--kind",
                kind,
                "--title",
                "Study",
                "--body",
                "Body",
                "--request-key",
                kind,
            ]);
            if let Some(status) = status {
                create.args(["--status", status]);
            }
            fixture.ok(&mut create);
        }
        fixture
    }

    fn command(&self, executable: &Path) -> Command {
        let mut paths = vec![
            self.orbit
                .parent()
                .expect("Orbit bin directory")
                .to_path_buf(),
        ];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut command = Command::new(executable);
        command
            .env_clear()
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("ORBIT_BIN", &self.orbit)
            // A cleared shell has no caller identity; use Orbit's audited operator override.
            .env("ORBIT_OPERATOR", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Installed Orbit fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Installed Orbit fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("PATH", std::env::join_paths(paths).expect("fixture PATH"));
        command
    }

    fn orbit_command(&self) -> Command {
        self.command(&self.orbit)
    }

    fn ok(&self, command: &mut Command) -> Output {
        let output = command.output().expect("run fixture command");
        assert!(output.status.success(), "{command:?}: {output:?}");
        output
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.ok(self.command(Path::new("git")).args(args));
        String::from_utf8(output.stdout)
            .expect("Git text")
            .trim()
            .to_owned()
    }

    fn tool(&self, name: &str, input: Value) -> Value {
        let output = self.ok(self.orbit_command().args([
            "tool",
            "run",
            name,
            "--input",
            &input.to_string(),
        ]));
        serde_json::from_slice(&output.stdout).expect("Orbit tool JSON")
    }

    /// A task `accept` would accept: delivered, in `review`, with a run.
    fn delivered_task(&self) -> String {
        let task = self.tool(
            "orbit.task.add",
            json!({"workspace": WORKSPACE, "title": "Investigate R001", "description": "Study",
                   "complexity": "low", "acceptance_criteria": ["Deliver R001"]}),
        );
        let id = task["id"].as_str().expect("task id").to_owned();
        for mut update in [
            json!({"status": "backlog"}),
            json!({"status": "in-progress", "plan": "Investigate"}),
            json!({"status": "review", "job_run_id": "run-1"}),
        ] {
            update["id"] = json!(id);
            self.tool("orbit.task.update", update);
        }
        id
    }

    fn readme(&self) -> String {
        self.git(&["ls-files", "research/*/README.md"])
    }

    /// Link R001 to `task` in the delivered README, as the run's commit would.
    fn deliver(&self, task: &str, edit: impl Fn(&str) -> String) {
        let path = self.root.join(self.readme());
        let text = fs::read_to_string(&path).expect("R README");
        let (head, rest) = text.split_once("\n---\n").expect("frontmatter");
        let head = head
            .lines()
            .filter(|line| !line.starts_with("orbit:"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            &path,
            edit(&format!(
                "{head}\norbit: {{task: {task}, run: run-1}}\n---\n{rest}"
            )),
        )
        .expect("deliver R");
        self.git(&["commit", "-qam", "Deliver R001"]);
    }

    /// Store `research-acceptance.json` as `accept` does, through Orbit.
    fn accept(&self, task: &str) {
        let artifact = json!({
            "research_id": "R001",
            "commit": self.git(&["rev-parse", "HEAD"]),
            "blob": self.git(&["rev-parse", &format!("HEAD:{}", self.readme())]),
            "run_id": "run-1",
            "artifact_digests": {},
        });
        let scratch = self.root.join(".orbit/tmp");
        fs::create_dir_all(&scratch).expect("Orbit scratch directory");
        let source = scratch.join("research-acceptance.json");
        fs::write(&source, artifact.to_string()).expect("stage artifact");
        self.tool(
            "orbit.task.artifact.put",
            json!({"id": task, "source_path": source, "path": "research-acceptance.json"}),
        );
    }

    fn assess(&self, orbit: &Path) -> Output {
        let hypothesis = self.git(&["rev-parse", "HEAD:hypotheses/H001-study.md"]);
        let mut command = self.command(Path::new(BINARY));
        command.env("ORBIT_BIN", orbit).args([
            "--json",
            "research",
            "assess",
            "--corpus",
            self.root.to_str().expect("UTF-8 path"),
            "--id",
            "H001",
            "--expected-blob",
            &hypothesis,
            "--research",
            "R001",
            "--revision",
            "1",
            "--verdict",
            "inconclusive",
            "--strength",
            "anecdote",
        ]);
        command.output().expect("assess through CLI")
    }

    fn refused(&self, orbit: &Path) -> String {
        let head = self.git(&["rev-parse", "HEAD"]);
        let output = self.assess(orbit);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let error: Value = serde_json::from_slice(&output.stderr).expect("CLI error JSON");
        assert_eq!(
            self.git(&["rev-parse", "HEAD"]),
            head,
            "refusal writes nothing"
        );
        error["error"]["message"]
            .as_str()
            .expect("diagnostic")
            .to_owned()
    }
}

#[test]
#[ignore = "requires ORBIT_RESEARCH_TEST_ORBIT_BIN, a real Orbit binary"]
fn assess_verifies_acceptance_stored_on_a_real_orbit_task() {
    let fixture = Fixture::new();
    let task = fixture.delivered_task();
    fixture.deliver(&task, str::to_owned);

    // Delivered and linked, but never accepted.
    let message = fixture.refused(&fixture.orbit);
    assert!(
        message.contains("no `research-acceptance.json`"),
        "{message}"
    );

    // Orbit cannot be started.
    let message = fixture.refused(&fixture.root.join("no-such-orbit"));
    assert!(message.contains("ORBIT_BIN"), "{message}");

    // Accepted: the stored artifact names the README at HEAD.
    fixture.accept(&task);
    let output = fixture.assess(&fixture.orbit);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.git(&["status", "--porcelain"]), "");

    // The README changes after acceptance.
    fixture.deliver(&task, |text| format!("{text}\nAmended after acceptance.\n"));
    let message = fixture.refused(&fixture.orbit);
    assert!(message.contains("changed after task"), "{message}");

    // A different task with no artifact.
    let other = fixture.delivered_task();
    fixture.deliver(&other, str::to_owned);
    let message = fixture.refused(&fixture.orbit);
    assert!(
        message.contains("no `research-acceptance.json`") && message.contains(&other),
        "{message}"
    );
}
