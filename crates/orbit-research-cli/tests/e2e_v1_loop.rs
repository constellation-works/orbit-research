//! The v1 research loop, end to end, on a disposable corpus.
//!
//! One corpus is created by `workspace init` from the bundled schema in a
//! private HOME and Orbit root, the canonical plugin is installed into that
//! root and enabled, and the loop then runs through the real installed plugin
//! and the real `orbit run job research_investigation`. It covers the ten
//! scenarios of the plugin spec's "Acceptance scenarios for v1" in this
//! order: capture-and-return, reserve-and-link, investigation, validation gate
//! (alongside the negative result's delivery), acceptance, negative result,
//! revised hypothesis, concurrency, sandbox and corpus independence, with the
//! four dashboard panels read before acceptance and again against the state the
//! loop produced. Every assertion and log line is prefixed with its scenario
//! name, and the test is meant to run with `--nocapture` so the evidence reads
//! top to bottom.
//!
//! The investigation agent is scripted, not a provider. Orbit offers a seam for
//! exactly this: an executor definition is a YAML file in the Orbit root
//! (`<root>/resources/executors/<name>.yaml`) whose `command` Orbit starts with
//! the prompt on stdin, the run worktree as working directory and the run's
//! `ORBIT_*` environment, and whose stdout it reads for the response envelope.
//! Orbit's own executor tests substitute a fake CLI the same way
//! (`crates/orbit-core/tests/pi_fake_agent.rs`, and
//! `application/job/tests/exec.rs::write_fake_codex`; the contract is described
//! in `docs/runbooks/executor-onboarding.md`). This test points the shipped
//! `codex` executor at `tests/fixtures/e2e_agent.sh` and makes the `sol` crew
//! the default, so the job's own `research_investigate` agent_loop step,
//! `worktree_setup`, `plugin.tool_call` validate step, `git_commit`,
//! `git_merge` and `update_task` all run unmodified under Orbit. Codex's JSONL
//! shape is used because its terminal `agent_message` carries the response
//! envelope (`orbit-agent/src/providers/codex/codex_output.rs`). Nothing here
//! calls a provider.
//!
//! One deliberate difference from production: a directory install has no
//! verified first-party origin (Orbit's `first_party_source` accepts only a
//! `git+` URL of a constellation-works repository), so its tools register as
//! `research.<verb>`, not `orbit.research.<verb>`. The export's
//! `research_validate` activity is rewritten to name `research.validate`; no
//! other plugin file differs from the repository.
#![cfg(unix)]
#![allow(clippy::print_stdout)]

mod common;

use common::{Installed, Mcp, json_output, mcp_output};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fmt::{Debug, Display};
use std::fs;
use std::io::Read;
use std::ops::Deref;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const WORKSPACE: &str = "research-e2e";
const QUESTION: &str = "Does the mean of a small sample agree with its shuffled control?";
/// Generous for a scripted run; a hang must fail the test, not the CI job.
const JOB_LIMIT: Duration = Duration::from_secs(600);

/// Readable evidence: each line names its scenario.
struct Scenario(&'static str);

impl Scenario {
    fn note(&self, message: impl Display) {
        println!("[{}] {message}", self.0);
    }

    fn eq<T: PartialEq + Debug>(&self, what: &str, actual: T, expected: T) {
        assert_eq!(actual, expected, "[{}] {what}", self.0);
        self.note(format!("ok: {what} ({actual:?})"));
    }

    fn check(&self, what: &str, condition: bool, detail: impl Debug) {
        assert!(condition, "[{}] {what}: {detail:?}", self.0);
        self.note(format!("ok: {what}"));
    }

    fn contains(&self, what: &str, haystack: &str, needle: &str) {
        assert!(
            haystack.contains(needle),
            "[{}] {what}: expected `{needle}` in `{haystack}`",
            self.0
        );
        self.note(format!("ok: {what} (`{needle}`)"));
    }
}

/// Run `command` to completion or fail after `limit`. Output is collected
/// off-thread; a detached Orbit worker that keeps a pipe open cannot hold the
/// test up, because collection stops shortly after the child exits.
fn bounded(mut command: Command, limit: Duration) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start bounded command");
    let collect = |mut pipe: Box<dyn Read + Send>| {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&bytes);
        let reader = thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .expect("output sink")
                    .extend_from_slice(&chunk[..read]);
            }
        });
        (bytes, reader)
    };
    let (stdout, stdout_reader) = collect(Box::new(child.stdout.take().expect("stdout pipe")));
    let (stderr, stderr_reader) = collect(Box::new(child.stderr.take().expect("stderr pipe")));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll command") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("command exceeded {limit:?}: {command:?}");
        }
        thread::sleep(Duration::from_millis(100));
    };
    let grace = Instant::now() + Duration::from_secs(5);
    while Instant::now() < grace && !(stdout_reader.is_finished() && stderr_reader.is_finished()) {
        thread::sleep(Duration::from_millis(50));
    }
    let take = |bytes: &Arc<Mutex<Vec<u8>>>| bytes.lock().expect("collected output").clone();
    Output {
        status,
        stdout: take(&stdout),
        stderr: take(&stderr),
    }
}

/// What `orbit run job --wait --json` and `orbit run show` report for one run.
struct JobRun {
    id: String,
    state: String,
    error: String,
    /// `(step id, state, error message)` as `orbit run show` lists them. A failed
    /// run lists only the job-level failure, so step outcomes are read from the
    /// `pipeline` outputs of the steps that completed.
    steps: Vec<(String, String, String)>,
    /// Outputs of the completed steps, keyed by step id.
    pipeline: Value,
}

impl JobRun {
    fn summary(&self) -> String {
        let steps = self
            .steps
            .iter()
            .map(|(id, state, _)| format!("{id}={state}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("run {} is {} [{steps}]", self.id, self.state)
    }
}

/// The installed-plugin fixture plus the scripted agent and the helpers the
/// scenarios share.
struct Lab {
    installed: Installed,
    control: PathBuf,
    branch: String,
}

impl Deref for Lab {
    type Target = Installed;

    fn deref(&self) -> &Installed {
        &self.installed
    }
}

impl Lab {
    fn new() -> Self {
        let installed = Installed::new(WORKSPACE);
        // A directory install has no verified first-party origin, so its tools
        // register without the `orbit.` prefix; point the validate step at the
        // registered name. Nothing else in the plugin export is altered.
        let validate = installed
            .source
            .join(".orbit-plugin/definitions/activities/research_validate.yaml");
        let text = fs::read_to_string(&validate).expect("validate activity");
        assert!(text.contains("tool: orbit.research.validate"), "{text}");
        fs::write(
            &validate,
            text.replace("tool: orbit.research.validate", "tool: research.validate"),
        )
        .expect("local validate activity");
        installed.install();

        let lab = Self {
            control: installed.root.join("agent-control"),
            branch: installed
                .git_text(&["rev-parse", "--abbrev-ref", "HEAD"])
                .trim()
                .to_owned(),
            installed,
        };
        // A runner with no provider CLI leaves every crew disabled. Enable the
        // `sol` crew (the Codex lane) and make it the default so linked tasks
        // draw it.
        for args in [
            ["config", "set", "--global", "crews.sol.enabled", "true"],
            ["config", "set", "--global", "workflow.default_crew", "sol"],
        ] {
            let output = lab.command().args(args).output().expect("orbit config");
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
        // Orbit's own commit step runs in its worker, not under this test's
        // environment: give the disposable corpus a repository-local identity.
        for (key, value) in [
            ("user.name", "E2E research fixture"),
            ("user.email", "fixture@example.invalid"),
        ] {
            lab.git_text(&["config", key, value]);
        }
        lab.install_scripted_agent();
        lab
    }

    /// Point the shipped `codex` executor at the scripted agent.
    fn install_scripted_agent(&self) {
        let bin = self.root.join("agent-bin");
        fs::create_dir_all(&bin).expect("agent directory");
        fs::create_dir_all(&self.control).expect("control directory");
        let script = include_str!("fixtures/e2e_agent.sh")
            .replace("@RESEARCH_BIN@", common::BINARY)
            .replace("@CONTROL_DIR@", self.control.to_str().expect("UTF-8 path"));
        let agent = bin.join("codex");
        common::exec_support::install_executable(&agent, &script);
        let executor = self.home.join(".orbit/resources/executors/codex.yaml");
        let text = fs::read_to_string(&executor).expect("seeded codex executor");
        assert!(text.contains("command: codex\n"), "{text}");
        assert!(!text.contains("allow_fallback"), "{text}");
        // A runner without `/usr/bin/bwrap` (GitHub's Ubuntu image) cannot start
        // the agent sandbox; `allow_fallback` permits bare exec only in that case
        // and keeps the sandbox wherever it is available.
        let text = text.replace(
            "command: codex\n",
            &format!("command: {}\n", agent.to_str().expect("UTF-8 path")),
        );
        fs::write(
            &executor,
            format!(
                "{}  allow_fallback: true\n",
                text.trim_end().to_owned() + "\n"
            ),
        )
        .expect("scripted executor");
    }

    fn corpus(&self) -> &str {
        self.repository.to_str().expect("UTF-8 corpus path")
    }

    fn head(&self) -> String {
        self.git_text(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn porcelain(&self) -> String {
        self.git_text(&["status", "--porcelain", "--untracked-files=all"])
    }

    /// `orbit-research research ...` against the corpus, as an operator.
    fn research_command(&self, args: &[&str]) -> Command {
        let mut command = self.installed.research_command();
        command
            .env("ORBIT_OPERATOR", "1")
            .arg("--json")
            .args(["research", args[0], "--corpus", self.corpus()])
            .args(&args[1..]);
        command
    }

    fn research(&self, args: &[&str]) -> Value {
        let output = self.research_command(args).output().expect("run writer");
        assert!(output.status.success(), "{args:?}: {output:?}");
        serde_json::from_slice(&output.stdout).expect("writer JSON")
    }

    /// A refused write: exit 1 with the message, writing nothing.
    fn research_refusal(&self, args: &[&str]) -> String {
        let head = self.head();
        let output = self.research_command(args).output().expect("run writer");
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let error: Value = serde_json::from_slice(&output.stderr).expect("writer error JSON");
        assert_eq!(self.head(), head, "{args:?}: a refusal writes nothing");
        error["error"]["message"]
            .as_str()
            .expect("error message")
            .to_owned()
    }

    fn orbit_tool(&self, name: &str, input: &Value) -> Value {
        let output = self
            .command()
            .env("ORBIT_OPERATOR", "1")
            .args(["tool", "run", name, "--input", &input.to_string()])
            .output()
            .expect("orbit tool");
        json_output(output, name)
    }

    fn plugin(&self, verb: &str, input: &Value) -> Value {
        json_output(self.cli_tool(verb, input), verb)
    }

    /// A plugin refusal: the structured error object `{code, message, ...}`.
    fn plugin_refusal(&self, verb: &str, input: &Value) -> Value {
        let output = self.cli_tool(verb, input);
        assert!(!output.status.success(), "{verb}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        serde_json::from_str(stderr.lines().last().expect("error JSON line"))
            .expect("plugin error object")
    }

    fn panel(&self, verb: &str) -> Value {
        self.plugin(verb, &json!({}))
    }

    fn readme_path(&self, id: &str) -> String {
        self.research(&["show", "--id", id])["path"]
            .as_str()
            .expect("record path")
            .to_owned()
    }

    fn blob(&self, id: &str) -> String {
        self.research(&["show", "--id", id])["git_blob"]
            .as_str()
            .expect("git blob")
            .to_owned()
    }

    fn metadata(&self, id: &str) -> Value {
        self.research(&["show", "--id", id])["metadata"].clone()
    }

    fn create(&self, kind: &str, title: &str, derived: &[&str], key: &str) -> String {
        let mut args = vec![
            "create",
            "--kind",
            kind,
            "--title",
            title,
            "--body",
            "Reserved for the end-to-end loop",
            "--request-key",
            key,
        ];
        if kind == "R" {
            args.extend(["--status", "planned"]);
        }
        for id in derived {
            args.extend(["--derived-from", id]);
        }
        let created = self.research(&args);
        assert_eq!(created["mode"], "primary", "{created}");
        created["id"].as_str().expect("record id").to_owned()
    }

    /// Draft with `plan`, then `link`, as an operator would.
    fn link(&self, research: &str) -> Value {
        let plan = self.plugin(
            "plan",
            &json!({"shape": "investigation", "research_id": research,
                    "objective": "Compare the sample mean with its control"}),
        );
        self.plugin(
            "link",
            &json!({"research_id": research, "request_key": format!("link-{research}"),
                    "title": plan["title"], "description": plan["description"],
                    "acceptance_criteria": plan["acceptance_criteria"]}),
        )
    }

    fn tasks(&self) -> Vec<Value> {
        let output = self
            .command()
            .env("ORBIT_OPERATOR", "1")
            .args(["task", "list", "--workspace", WORKSPACE, "--json"])
            .output()
            .expect("orbit task list");
        json_output(output, "task list")
            .as_array()
            .expect("task records")
            .clone()
    }

    fn task_status(&self, task: &str) -> String {
        self.orbit_tool(
            "orbit.task.show",
            &json!({"id": task, "fields": ["status", "job_run_id"]}),
        )["status"]
            .as_str()
            .expect("task status")
            .to_owned()
    }

    fn approve(&self, task: &str) {
        self.orbit_tool(
            "orbit.task.update",
            &json!({"id": task, "status": "backlog"}),
        );
    }

    /// Run the real job. `variant` steers what the scripted agent writes.
    fn run_job(&self, task: &str, research: &str, variant: &str) -> JobRun {
        fs::write(self.control.join(task), format!("{variant} {research}\n")).expect("control");
        let mut command = self.command();
        command
            .env("ORBIT_OPERATOR", "1")
            .args([
                "run",
                "job",
                "research_investigation",
                "--workspace",
                WORKSPACE,
            ])
            .args(["--input", &format!("task={task}")])
            .args(["--input", &format!("base_branch={}", self.branch)])
            .args(["--wait", "--json"]);
        let output = bounded(command, JOB_LIMIT);
        let raw: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("run job output is not JSON ({error}): {output:?}"));
        let id = raw["run_id"].as_str().expect("run id").to_owned();
        let shown = json_output(
            self.command()
                .env("ORBIT_OPERATOR", "1")
                .args(["run", "show", &id, "--json"])
                .output()
                .expect("orbit run show"),
            "run show",
        );
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        JobRun {
            state: text(&raw["state"]),
            error: text(&raw["error"]),
            steps: shown["steps"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|step| {
                    (
                        text(&step["target_id"]),
                        text(&step["state"]),
                        text(&step["error_message"]),
                    )
                })
                .collect(),
            pipeline: raw["pipeline"].clone(),
            id,
        }
    }

    fn files_in_head(&self) -> BTreeSet<String> {
        self.git_text(&["show", "--name-only", "--format=", "HEAD"])
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A panel's rows without the volatile date columns.
fn stable(rows: &Value) -> Value {
    Value::Array(
        rows.as_array()
            .expect("table panel rows")
            .iter()
            .map(|row| {
                let mut row = row.clone();
                for date in ["updated", "when"] {
                    row.as_object_mut().expect("row").remove(date);
                }
                row
            })
            .collect(),
    )
}

fn status_row(rows: &Value) -> String {
    let rows = rows.as_array().expect("table panel rows");
    assert_eq!(rows.len(), 1, "one readable status row: {rows:?}");
    rows[0]["status"].as_str().expect("status text").to_owned()
}

#[test]
#[ignore = "requires ORBIT_RESEARCH_TEST_ORBIT_BIN and its native plugin sandbox"]
fn v1_research_loop_on_a_disposable_corpus() {
    let lab = Lab::new();
    let tasks_before = lab.tasks();
    assert!(tasks_before.is_empty(), "a fresh Orbit root has no tasks");
    assert_eq!(lab.porcelain(), "", "the corpus starts clean");

    // ---- 1. capture and return -------------------------------------------
    let s = Scenario("capture-and-return");
    let captured = lab.research(&["capture", "--text", QUESTION, "--tag", "e2e"]);
    s.eq(
        "capture reserves Q001 in primary mode",
        captured["id"].clone(),
        json!("Q001"),
    );
    s.eq("capture mode", captured["mode"].clone(), json!("primary"));
    // Every call below is a new process: nothing is cached between them.
    let listed = lab.research(&["list"]);
    let questions: Vec<&Value> = listed["records"]
        .as_array()
        .expect("records")
        .iter()
        .filter(|record| record["kind"] == "Q")
        .collect();
    s.eq(
        "a fresh `list` process returns the question",
        questions.len(),
        1,
    );
    s.eq(
        "question title",
        questions[0]["metadata"]["title"].clone(),
        json!(QUESTION),
    );
    let via_plugin = lab.plugin("list", &json!({}));
    s.eq(
        "the installed plugin's `list` returns it too",
        via_plugin["records"][0]["id"].clone(),
        json!("Q001"),
    );
    s.eq(
        "capture created no Orbit task",
        (lab.tasks().len(), lab.research(&["work-links"])),
        (0, json!([])),
    );
    s.eq(
        "panel open-questions shows it with no task",
        stable(&lab.panel("open-questions")),
        json!([{"id": "Q001", "question": QUESTION, "tags": "e2e", "tasks": null}]),
    );
    s.contains(
        "panel awaiting-acceptance starts empty",
        &status_row(&lab.panel("awaiting-acceptance")),
        "No delivered results yet",
    );
    s.contains(
        "panel hypotheses starts empty",
        &status_row(&lab.panel("hypotheses")),
        "No hypotheses yet",
    );
    let health = lab.panel("corpus-health");
    s.eq(
        "panel corpus-health is valid",
        health["Corpus"].clone(),
        json!("Valid"),
    );
    s.eq(
        "panel corpus-health counts the question",
        health["Questions"].clone(),
        json!("1 (1 open)"),
    );
    assert_eq!(lab.porcelain(), "", "capture leaves the corpus clean");

    // ---- 2. reserve and link ---------------------------------------------
    let s = Scenario("reserve-and-link");
    let h1 = lab.create("H", "The sample mean equals its control", &["Q001"], "h1");
    let h2 = lab.create("H", "The control is unbiased", &["Q001"], "h2");
    s.eq("hypotheses", (h1.as_str(), h2.as_str()), ("H001", "H002"));
    // R001 positive, R002 failed controls, R003..R006 the four gate failures,
    // R007 never dispatched.
    let plan: Vec<(&str, &str, &str, &str)> = vec![
        ("R001", "valid", "H001", "Positive result"),
        ("R002", "failed_controls", "H002", "Negative result"),
        ("R003", "missing_section", "H001", "Missing section"),
        ("R004", "wrong_run", "H001", "Wrong run id"),
        ("R005", "tampered_digest", "H001", "Tampered digest"),
        ("R006", "allocated_id", "H001", "Allocated id"),
        ("R007", "never_run", "H001", "Never dispatched"),
    ];
    let mut task_of = std::collections::BTreeMap::new();
    for (research, _, hypothesis, title) in &plan {
        let id = lab.create(
            "R",
            title,
            &["Q001", *hypothesis],
            &format!("reserve-{research}"),
        );
        s.eq("reserved id", id.as_str(), *research);
        let stub = lab.metadata(research);
        s.eq(
            &format!("{research} is a planned stub"),
            stub["status"].clone(),
            json!("planned"),
        );
        let created = lab.link(research);
        s.eq(
            &format!("link {research} creates a task"),
            created["created"].clone(),
            json!(true),
        );
        let task = created["task_id"].as_str().expect("task id").to_owned();
        task_of.insert(*research, task);
    }
    s.eq(
        "exactly one task per reserved record",
        lab.tasks().len(),
        plan.len(),
    );
    let r1_task = task_of["R001"].clone();
    let retry = lab.link("R001");
    s.eq(
        "identical retry adopts the task",
        retry["created"].clone(),
        json!(false),
    );
    s.eq(
        "retry names the same task",
        retry["task_id"].clone(),
        json!(r1_task),
    );
    s.eq("the retry created nothing", lab.tasks().len(), plan.len());
    let shown = lab.orbit_tool(
        "orbit.task.show",
        &json!({"id": r1_task, "fields": ["status", "tags", "context_files", "resolved_crew"]}),
    );
    s.eq(
        "linked task starts proposed",
        shown["status"].clone(),
        json!("proposed"),
    );
    s.eq(
        "context_files names the reserved record",
        shown["context_files"].clone(),
        json!([format!(
            "dir:{}",
            lab.readme_path("R001").trim_end_matches("/README.md")
        )]),
    );
    s.check(
        "the task carries its request tag",
        shown["tags"].to_string().contains("research-request:"),
        &shown["tags"],
    );
    s.eq(
        "the task drew the scripted crew",
        shown["resolved_crew"].clone(),
        json!("sol"),
    );
    s.eq(
        "panel hypotheses lists both as not assessed",
        stable(&lab.panel("hypotheses"))
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                (
                    row["id"].clone(),
                    row["revision"].clone(),
                    row["verdict"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (json!("H001"), json!("1 (current)"), json!("not assessed")),
            (json!("H002"), json!("1 (current)"), json!("not assessed")),
        ],
    );
    assert_eq!(lab.porcelain(), "", "linking keeps the corpus clean");

    // ---- 3. investigation (the real job, a scripted agent) ---------------
    let s = Scenario("investigation");
    let base = lab.head();
    let t1 = task_of["R001"].clone();
    lab.approve(&t1);
    let run1 = lab.run_job(&t1, "R001", "valid");
    s.note(run1.summary());
    s.eq(
        "the run succeeds",
        (run1.state.as_str(), run1.error.as_str()),
        ("success", ""),
    );
    for step in [
        "worktree",
        "investigate",
        "validate",
        "commit",
        "merge",
        "mark_review",
    ] {
        s.check(
            &format!("step `{step}` completed"),
            run1.pipeline.get(step).is_some(),
            run1.pipeline
                .as_object()
                .map(|steps| steps.keys().collect::<Vec<_>>()),
        );
    }
    let validated = run1.pipeline["validate"].to_string();
    s.note(format!("validate step output: {validated}"));
    s.contains(
        "the validate gate named the delivered record",
        &validated,
        "R001",
    );
    s.eq(
        "the task reached review",
        lab.task_status(&t1),
        "review".to_owned(),
    );
    s.eq(
        "the task records the delivering run",
        lab.orbit_tool(
            "orbit.task.show",
            &json!({"id": t1, "fields": ["status", "job_run_id"]}),
        )["job_run_id"]
            .clone(),
        json!(run1.id),
    );
    s.eq(
        "the agent ran under the codex executor lane",
        run1.pipeline["investigate"]["provider"].clone(),
        json!("codex"),
    );
    s.eq(
        "the agent reported the reserved record",
        run1.pipeline["investigate"]["research_id"].clone(),
        json!("R001"),
    );
    let r1 = lab.metadata("R001");
    s.eq("R001 is done", r1["status"].clone(), json!("done"));
    s.eq(
        "R001 frontmatter binds the task and run",
        r1["orbit"].clone(),
        json!({"task": t1, "run": run1.id}),
    );
    s.check(
        "the delivery landed on the base branch",
        lab.head() != base,
        lab.head(),
    );
    s.eq(
        "the delivery commit's files",
        lab.files_in_head(),
        [
            "research/R001-positive-result/README.md",
            "research/R001-positive-result/artifacts/result.json",
            "research/R001-positive-result/code/experiment.sh",
            "research/R001-positive-result/data/manifest.json",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    );
    s.eq(
        "input bytes stay uncommitted",
        lab.git_text(&["ls-files", "research/R001-positive-result/data/sample.csv"]),
        String::new(),
    );
    let readme = fs::read_to_string(
        lab.repository
            .join("research/R001-positive-result/README.md"),
    )
    .expect("delivered README");
    s.contains(
        "the README has the Result section",
        &readme,
        "the control passed",
    );
    let check = lab.plugin("check", &json!({}));
    s.eq(
        "the delivered corpus checks valid",
        check["valid"].clone(),
        json!(true),
    );
    s.eq(
        "the checkout is clean after delivery",
        lab.porcelain(),
        String::new(),
    );

    // ---- 6a. negative result: failed controls still deliver -------------
    let s6 = Scenario("negative-result");
    let t2 = task_of["R002"].clone();
    lab.approve(&t2);
    let run2 = lab.run_job(&t2, "R002", "failed_controls");
    s6.note(run2.summary());
    s6.eq(
        "failed controls still deliver",
        (run2.state.as_str(), run2.error.as_str()),
        ("success", ""),
    );
    s6.eq(
        "the task reached review",
        lab.task_status(&t2),
        "review".to_owned(),
    );
    let readme = fs::read_to_string(
        lab.repository
            .join("research/R002-negative-result/README.md"),
    )
    .expect("delivered negative README");
    s6.contains(
        "the Result says the control failed",
        &readme,
        "the control failed",
    );
    s6.contains("the Result says inconclusive", &readme, "inconclusive");

    // ---- 4. validation gate ----------------------------------------------
    let s = Scenario("validation-gate");
    let delivered_head = lab.head();
    for (research, variant, reason) in [
        ("R003", "missing_section", "section_missing"),
        ("R004", "wrong_run", "orbit_run_mismatch"),
        ("R005", "tampered_digest", "artifact_digest_mismatch"),
        ("R006", "allocated_id", "id_allocated"),
    ] {
        let task = task_of[research].clone();
        lab.approve(&task);
        let run = lab.run_job(&task, research, variant);
        s.note(format!("{variant}: {}", run.summary()));
        s.eq(
            &format!("{variant}: the run fails"),
            run.state.as_str(),
            "failed",
        );
        let failure = run.error.clone();
        s.note(format!("{variant}: run error: {failure}"));
        s.contains(
            &format!("{variant}: the plugin.tool_call validate step failed"),
            &failure,
            "plugin.tool_call",
        );
        s.contains(
            &format!("{variant}: the finding names its reason"),
            &failure,
            reason,
        );
        s.check(
            &format!("{variant}: the agent ran and commit never did"),
            run.pipeline.get("investigate").is_some() && run.pipeline.get("commit").is_none(),
            run.pipeline
                .as_object()
                .map(|steps| steps.keys().collect::<Vec<_>>()),
        );
        s.eq(
            &format!("{variant}: nothing landed"),
            lab.head(),
            delivered_head.clone(),
        );
        let status = lab.task_status(&task);
        s.check(
            &format!("{variant}: the task did not reach review"),
            status != "review" && status != "done",
            &status,
        );
        s.eq(
            &format!("{variant}: the corpus is clean"),
            lab.porcelain(),
            String::new(),
        );
    }

    // ---- panels after delivery, before acceptance ------------------------
    let s = Scenario("panels-before-acceptance");
    let awaiting = lab.panel("awaiting-acceptance");
    s.eq(
        "both delivered results await acceptance",
        stable(&awaiting)
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                (
                    row["id"].clone(),
                    row["status"].clone(),
                    row["task"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (json!("R001"), json!("awaiting acceptance"), json!(t1)),
            (json!("R002"), json!("awaiting acceptance"), json!(t2)),
        ],
    );
    let mut delivered_tasks = [t1.clone(), t2.clone()];
    delivered_tasks.sort();
    s.eq(
        "open-questions lists the delivered tasks",
        stable(&lab.panel("open-questions"))[0]["tasks"].clone(),
        json!(delivered_tasks.join(", ")),
    );

    // ---- 5. acceptance ----------------------------------------------------
    let s = Scenario("acceptance");
    for (research, why) in [
        ("R003", "a run that failed validation"),
        ("R007", "a task never dispatched"),
    ] {
        let task = task_of[research].clone();
        let refused =
            lab.plugin_refusal("accept", &json!({"task_id": task, "research_id": research}));
        s.eq(
            &format!("accept refuses {why}"),
            refused["code"].clone(),
            json!("refused"),
        );
        s.contains(
            "the refusal explains the task state",
            refused["message"].as_str().unwrap_or_default(),
            "not `review` or `done`",
        );
        let artifacts = lab.orbit_tool(
            "orbit.task.show",
            &json!({"id": task, "fields": ["status", "artifacts"]}),
        );
        s.check(
            "the refused accept stored nothing",
            !artifacts.to_string().contains("research-acceptance.json"),
            &artifacts,
        );
    }
    let refused = lab.research_refusal(&[
        "assess",
        "--id",
        "H001",
        "--expected-blob",
        &lab.blob("H001"),
        "--research",
        "R001",
        "--revision",
        "1",
        "--verdict",
        "supports",
        "--strength",
        "suggestive",
    ]);
    s.contains(
        "assess refuses a delivered but unaccepted result",
        &refused,
        "research-acceptance.json",
    );

    let accepted = lab.plugin("accept", &json!({"task_id": t1, "research_id": "R001"}));
    s.eq(
        "accept records the evidence",
        accepted["recorded"].clone(),
        json!(true),
    );
    s.eq(
        "accepted run id",
        accepted["run_id"].clone(),
        json!(run1.id),
    );
    let commit = lab.head();
    s.eq(
        "accepted commit is the published HEAD",
        accepted["commit"].clone(),
        json!(commit),
    );
    let r1_path = lab.readme_path("R001");
    s.eq(
        "accepted blob is the delivered README",
        accepted["blob"].clone(),
        json!(
            lab.git_text(&["rev-parse", &format!("HEAD:{r1_path}")])
                .trim()
        ),
    );
    let stored = lab.orbit_tool(
        "orbit.task.artifact.get",
        &json!({"id": t1, "path": "research-acceptance.json"}),
    );
    let stored: Value = serde_json::from_str(stored["content"].as_str().expect("artifact content"))
        .expect("acceptance JSON");
    s.eq(
        "research-acceptance.json is stored on the task",
        (
            stored["research_id"].clone(),
            stored["run_id"].clone(),
            stored["commit"].clone(),
            stored["blob"].clone(),
        ),
        (
            json!("R001"),
            json!(run1.id),
            json!(commit),
            accepted["blob"].clone(),
        ),
    );
    let again = lab.plugin("accept", &json!({"task_id": t1, "research_id": "R001"}));
    s.eq(
        "re-running accept is idempotent",
        again["recorded"].clone(),
        json!(false),
    );
    s.eq(
        "the retry returns the stored evidence",
        (
            again["commit"].clone(),
            again["blob"].clone(),
            again["run_id"].clone(),
        ),
        (
            accepted["commit"].clone(),
            accepted["blob"].clone(),
            accepted["run_id"].clone(),
        ),
    );
    s.eq(
        "accept left the corpus clean",
        lab.porcelain(),
        String::new(),
    );
    s.eq(
        "panel awaiting-acceptance drops the accepted result",
        stable(&lab.panel("awaiting-acceptance"))
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| (row["id"].clone(), row["status"].clone()))
            .collect::<Vec<_>>(),
        vec![(json!("R002"), json!("awaiting acceptance"))],
    );

    // ---- 6b. negative result: accept, then assess inconclusive ----------
    let accepted2 = lab.plugin("accept", &json!({"task_id": t2, "research_id": "R002"}));
    s6.eq(
        "the failed-controls result is accepted",
        accepted2["recorded"].clone(),
        json!(true),
    );
    s6.contains(
        "panel awaiting-acceptance is empty once both are accepted",
        &status_row(&lab.panel("awaiting-acceptance")),
        "Nothing awaiting acceptance. All 2 delivered results are accepted.",
    );
    lab.research(&[
        "assess",
        "--id",
        "H002",
        "--expected-blob",
        &lab.blob("H002"),
        "--research",
        "R002",
        "--revision",
        "1",
        "--verdict",
        "inconclusive",
        "--strength",
        "anecdote",
        "--note",
        "failed controls",
    ]);
    let h2_meta = lab.metadata("H002");
    s6.eq(
        "assess recorded inconclusive, never supports",
        h2_meta["assessments"]
            .as_array()
            .expect("assessments")
            .iter()
            .map(|entry| {
                (
                    entry["research"].clone(),
                    entry["verdict"].clone(),
                    entry["revision"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![(json!("R002"), json!("inconclusive"), json!(1))],
    );
    s6.eq(
        "H002 status follows the verdict",
        h2_meta["status"].clone(),
        json!("inconclusive"),
    );
    lab.research(&[
        "assess",
        "--id",
        "H001",
        "--expected-blob",
        &lab.blob("H001"),
        "--research",
        "R001",
        "--revision",
        "1",
        "--verdict",
        "supports",
        "--strength",
        "suggestive",
        "--note",
        "control passed",
    ]);
    s.eq(
        "assess after acceptance succeeds",
        lab.metadata("H001")["status"].clone(),
        json!("supported"),
    );
    s.eq(
        "assess leaves the corpus clean",
        lab.porcelain(),
        String::new(),
    );
    // The assess commits moved HEAD past the commit R001 was accepted at; a
    // retry with the same evidence is still idempotent and keeps that commit.
    s.check(
        "the assess commits moved HEAD past the accepted commit",
        lab.head() != commit,
        (lab.head(), &commit),
    );
    let after_assess = lab.plugin("accept", &json!({"task_id": t1, "research_id": "R001"}));
    s.eq(
        "re-running accept after later commits stays idempotent",
        after_assess["recorded"].clone(),
        json!(false),
    );
    s.eq(
        "the retry keeps the first accepted commit",
        after_assess["commit"].clone(),
        json!(commit),
    );

    // ---- 7. revised hypothesis -------------------------------------------
    let s = Scenario("revised-hypothesis");
    let old_h1_blob = lab.blob("H001");
    let revised = lab.research(&[
        "revise",
        "--id",
        "H001",
        "--expected-blob",
        &old_h1_blob,
        "--body",
        "The sample mean equals its control on any resample.",
    ]);
    s.eq(
        "the revision is committed in primary mode",
        revised["mode"].clone(),
        json!("primary"),
    );
    let h1_meta = lab.metadata("H001");
    s.eq(
        "the revision bumped to 2",
        h1_meta["revision"].clone(),
        json!(2),
    );
    s.eq(
        "the status reopened",
        h1_meta["status"].clone(),
        json!("open"),
    );
    s.eq(
        "the earlier assessment stays on revision 1",
        h1_meta["assessments"]
            .as_array()
            .expect("assessments")
            .iter()
            .map(|entry| {
                (
                    entry["revision"].clone(),
                    entry["verdict"].clone(),
                    entry["research"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![(json!(1), json!("supports"), json!("R001"))],
    );
    let refused = lab.research_refusal(&[
        "assess",
        "--id",
        "H001",
        "--expected-blob",
        &lab.blob("H001"),
        "--research",
        "R001",
        "--revision",
        "3",
        "--verdict",
        "supports",
        "--strength",
        "anecdote",
    ]);
    s.contains(
        "assess against a missing revision refuses",
        &refused,
        "no revision 3",
    );

    // ---- 8. concurrency ---------------------------------------------------
    let s = Scenario("concurrency");
    let before = lab.head();
    let writers: Vec<Value> = thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|index| {
                let lab = &lab;
                scope.spawn(move || {
                    lab.research(&[
                        "capture",
                        "--text",
                        &format!("Concurrent question {index}?"),
                        "--tag",
                        "e2e",
                        "--request-key",
                        &format!("concurrent-{index}"),
                    ])
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("writer thread"))
            .collect()
    });
    let ids: BTreeSet<String> = writers
        .iter()
        .map(|created| created["id"].as_str().expect("id").to_owned())
        .collect();
    s.eq(
        "four simultaneous primary writes get distinct ids",
        ids,
        ["Q002", "Q003", "Q004", "Q005"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    );
    s.eq(
        "each write is its own commit",
        lab.git_text(&["rev-list", "--count", &format!("{before}..HEAD")])
            .trim()
            .to_owned(),
        "4".to_owned(),
    );
    s.eq(
        "the corpus is clean after the race",
        lab.porcelain(),
        String::new(),
    );
    let refused = lab.research_refusal(&[
        "revise",
        "--id",
        "H001",
        "--expected-blob",
        &old_h1_blob,
        "--body",
        "A stale edit.",
    ]);
    s.contains(
        "a stale expected blob refuses",
        &refused,
        "changed since it was opened",
    );

    // ---- 9. sandbox -------------------------------------------------------
    let s = Scenario("sandbox");
    let backend = &lab.manifest["spec"]["backend"];
    s.eq(
        "the backend runs in Orbit's default sandbox",
        backend["sandbox"].clone(),
        json!("default"),
    );
    let manifest_text =
        fs::read_to_string(lab.source.join(".orbit-plugin/plugin.yaml")).expect("manifest");
    s.check(
        "the manifest names no unsandboxed grant",
        !manifest_text.contains("unsandboxed"),
        "unsandboxed",
    );
    s.check(
        "the manifest declares no requires.programs",
        lab.manifest["spec"]["requires"].get("programs").is_none(),
        &lab.manifest["spec"]["requires"],
    );
    let doctor = lab
        .command()
        .args(["plugin", "doctor", "--format", "json"])
        .output()
        .expect("orbit plugin doctor");
    s.check(
        "plugin doctor exits clean",
        doctor.status.success(),
        &doctor,
    );
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor JSON");
    let research = report
        .as_array()
        .expect("doctor rows")
        .iter()
        .find(|row| row["plugin"] == "research")
        .expect("a doctor row for the research plugin");
    s.note(format!("doctor: {research}"));
    s.eq(
        "doctor has nothing to report for research",
        research["message"].clone(),
        json!(""),
    );

    // ---- 10. corpus independence -----------------------------------------
    let s = Scenario("corpus-independence");
    let tracked = lab.git_text(&["ls-files"]);
    let allowed = [
        ".gitignore",
        "README.md",
        "_scripts/",
        "hypotheses/",
        "questions/",
        "research/",
        "theories/",
    ];
    let strays: Vec<&str> = tracked
        .lines()
        .filter(|path| !allowed.iter().any(|prefix| path.starts_with(prefix)))
        .collect();
    s.eq(
        "only owner-layout paths are tracked",
        strays,
        Vec::<&str>::new(),
    );
    let mut mentions = BTreeSet::new();
    for path in tracked.lines() {
        let text = fs::read_to_string(lab.repository.join(path))
            .unwrap_or_default()
            .to_lowercase();
        for word in [
            "nebula",
            "constellation",
            "knowledgebase",
            "codebases/",
            "neb_root",
        ] {
            if text.contains(word) {
                mentions.insert(format!("{path}: {word}"));
            }
        }
        // The bundled schema carries its own identity (`x-observatory`); no
        // record or other file may point at an Observatory checkout.
        if text.contains("observatory") && path != "_scripts/schema.json" {
            mentions.insert(format!("{path}: observatory"));
        }
    }
    s.eq(
        "no tracked file names Observatory, Nebula or a sibling path",
        mentions,
        BTreeSet::new(),
    );
    s.eq(
        "the corpus has no remote",
        lab.git_text(&["remote"]).trim().to_owned(),
        String::new(),
    );
    let temp_root = lab.root.to_str().expect("UTF-8 root").to_owned();
    let worktrees = lab.git_text(&["worktree", "list", "--porcelain"]);
    s.check(
        "every worktree lives inside the disposable root",
        worktrees
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .all(|path| path.starts_with(&temp_root)),
        &worktrees,
    );
    s.eq(
        "the corpus was initialised from the bundled schema",
        fs::read(lab.repository.join("_scripts/schema.json")).expect("corpus schema"),
        fs::read(common::repository_root().join("crates/orbit-research-store/assets/schema.json"))
            .expect("bundled schema"),
    );

    // ---- panels: the resulting state, through the CLI and through MCP ----
    let s = Scenario("panels-after-loop");
    s.eq(
        "hypotheses: the revision history and verdicts",
        stable(&lab.panel("hypotheses"))
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                (
                    row["id"].as_str().unwrap_or_default().to_owned(),
                    row["revision"].as_str().unwrap_or_default().to_owned(),
                    row["status"].as_str().unwrap_or_default().to_owned(),
                    row["verdict"].as_str().unwrap_or_default().to_owned(),
                    row["research"].as_str().map(str::to_owned),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                "H001".to_owned(),
                "2 (current)".to_owned(),
                "open".to_owned(),
                "not assessed".to_owned(),
                None,
            ),
            (
                "H001".to_owned(),
                "1 (superseded)".to_owned(),
                "open".to_owned(),
                "supports (suggestive)".to_owned(),
                Some("R001".to_owned()),
            ),
            (
                "H002".to_owned(),
                "1 (current)".to_owned(),
                "inconclusive".to_owned(),
                "inconclusive (anecdote)".to_owned(),
                Some("R002".to_owned()),
            ),
        ],
    );
    let health = lab.panel("corpus-health");
    s.eq(
        "corpus-health: valid",
        health["Corpus"].clone(),
        json!("Valid"),
    );
    s.eq(
        "corpus-health: questions",
        health["Questions"].clone(),
        json!("5 (5 open)"),
    );
    s.eq(
        "corpus-health: hypotheses",
        health["Hypotheses"].clone(),
        json!("2 (1 open, 1 inconclusive)"),
    );
    s.eq(
        "corpus-health: results",
        health["Results"].clone(),
        json!("7 (5 planned, 2 done)"),
    );
    s.eq(
        "corpus-health: records",
        health["Records"].clone(),
        json!(14),
    );
    s.contains(
        "awaiting-acceptance: nothing left",
        &status_row(&lab.panel("awaiting-acceptance")),
        "All 2 delivered results are accepted",
    );
    s.eq(
        "open-questions: Q001 carries the two delivered tasks, the new questions none",
        stable(&lab.panel("open-questions"))
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| (row["id"].clone(), row["tasks"].clone()))
            .collect::<Vec<_>>(),
        vec![
            // Newest update first; all were updated today, so ids descend.
            (json!("Q005"), Value::Null),
            (json!("Q004"), Value::Null),
            (json!("Q003"), Value::Null),
            (json!("Q002"), Value::Null),
            (json!("Q001"), json!(delivered_tasks.join(", "))),
        ],
    );
    // The same four panels over MCP, without operator capability: the way a
    // dashboard reads them.
    let mut mcp = Mcp::start(&lab, false);
    for verb in [
        "open-questions",
        "awaiting-acceptance",
        "hypotheses",
        "corpus-health",
    ] {
        let result = mcp.call(verb, json!({}));
        s.check(
            &format!("MCP {verb} answers"),
            result["isError"] != true,
            &result,
        );
        s.eq(
            &format!("MCP {verb} equals the CLI answer"),
            stable_or_kv(&mcp_output(&result)),
            stable_or_kv(&lab.panel(verb)),
        );
    }
    assert_eq!(
        lab.porcelain(),
        "",
        "the whole loop leaves the corpus clean"
    );
    s.note("the v1 loop completed on the disposable corpus");
}

fn stable_or_kv(value: &Value) -> Value {
    if value.is_array() {
        stable(value)
    } else {
        value.clone()
    }
}
