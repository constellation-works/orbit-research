//! The plugin's `research_investigation` job, driven step by step with no
//! live provider and no Orbit installation.
//!
//! A small interpreter walks `jobs/research_investigation.yaml` in order,
//! renders each step's `default_input` the way Orbit's job executor does
//! (`{{ input.* }}`, `{{ steps.<id>.output.* }}`), and stops the run at the
//! first failed step. Shipped activities get scripted stand-ins over real Git;
//! the agent step is a scripted writer running this crate's binary in the run
//! worktree; the validate step runs the real plugin backend (`orbit-tool`)
//! with the envelope Orbit builds for a deterministic `plugin.tool_call` step.
//! Every corpus and worktree lives in temporary directories removed on drop.
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");
const TASK: &str = "RES-1";
const RUN: &str = "jrun-research-test";
/// The scripted agent's one input, and its SHA-256 as the manifest pins it.
const INPUT: &[u8] = b"t,angle\n0,0.10\n1,0.11\n";
const INPUT_SHA256: &str = "9ea273cc3d10c915cddf3a05d5375e2a5b5d64595aefcaf668cccf028e01a9e6";
/// Activities Orbit ships that this job may reference.
const SHIPPED: &[&str] = &[
    "worktree_setup",
    "git_commit",
    "git_merge",
    "update_task",
    "step_failure_recovery",
];

fn repo_file(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn yaml(path: &str) -> Value {
    let text = fs::read_to_string(repo_file(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn job() -> Value {
    yaml("jobs/research_investigation.yaml")
}

fn activity(name: &str) -> Value {
    yaml(&format!("activities/{name}.yaml"))
}

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env_remove("ORBIT_RESEARCH_FORMAT")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Research fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn checked(command: &mut Command) -> String {
    let output = command.output().expect("run fixture command");
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git(root: &Path, args: &[&str]) -> String {
    checked(command("git").arg("-C").arg(root).args(args))
}

fn research(args: &[&str]) -> Value {
    let stdout = checked(command(BINARY).arg("--json").args(args));
    serde_json::from_str(&stdout).expect("orbit-research JSON")
}

/// A corpus with a reserved `R001`, as `create --kind R --status planned` and
/// `link` leave it before dispatch.
struct Fixture {
    temp: TempDir,
    corpus: PathBuf,
    task_status: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture directory");
        let corpus = temp.path().join("corpus");
        let path = corpus.to_str().expect("UTF-8 path");
        research(&["workspace", "init", path]);
        research(&[
            "research",
            "create",
            "--corpus",
            path,
            "--kind",
            "R",
            "--status",
            "planned",
            "--title",
            "Drift",
            "--body",
            "Does the pendulum drift overnight?",
            "--request-key",
            "reserve-drift",
        ]);
        Self {
            temp,
            corpus,
            task_status: "todo".into(),
        }
    }

    fn head(&self) -> String {
        git(&self.corpus, &["rev-parse", "HEAD"])
    }

    fn base_branch(&self) -> String {
        git(&self.corpus, &["rev-parse", "--abbrev-ref", "HEAD"])
    }
}

/// What the scripted agent writes in the run worktree.
#[derive(Clone, Copy)]
enum Agent {
    /// A complete record under the run's own task and run IDs.
    Valid,
    /// The Result section still holds the stub's placeholder.
    Placeholder,
    /// Provenance names a different run.
    WrongRun,
}

impl Agent {
    /// The investigation, done the way the activity prompt instructs: read the
    /// stub, produce an input, and write README and manifest with the
    /// worktree-mode writer.
    fn investigate(self, scratch: &Path, input: &Value) {
        let worktree = input["workspace_path"].as_str().expect("workspace_path");
        let task = input["task_id"].as_str().expect("task_id");
        let run = match self {
            Self::WrongRun => "jrun-some-other-run",
            _ => input["job_run_id"].as_str().expect("job_run_id"),
        };
        let blob =
            research(&["research", "show", "--corpus", worktree, "--id", "R001"])["git_blob"]
                .as_str()
                .expect("git_blob")
                .to_owned();
        fs::write(
            Path::new(worktree).join("research/R001-drift/data/night.csv"),
            INPUT,
        )
        .expect("input bytes");
        let result = match self {
            Self::Placeholder => "Pending.",
            _ => {
                "The control pendulum drifted as much as the test one, so the run is inconclusive."
            }
        };
        let body = scratch.join("body.md");
        fs::write(
            &body,
            format!(
                "## Question\n\nDoes the pendulum drift overnight?\n\n## Method\n\nLogged both pendulums for one night.\n\n## Result\n\n{result}\n\n## Limitations\n\nOne night.\n\n## Next\n\nRepeat with a fixed control.\n"
            ),
        )
        .expect("body");
        let manifest = scratch.join("manifest.json");
        fs::write(
            &manifest,
            json!({"inputs": [{
                "name": "night.csv",
                "source": "bench logger",
                "sha256": INPUT_SHA256,
                "size": INPUT.len(),
                "fetch": "recorded locally",
            }]})
            .to_string(),
        )
        .expect("manifest");
        let written = research(&[
            "research",
            "revise",
            "--corpus",
            worktree,
            "--mode",
            "worktree",
            "--id",
            "R001",
            "--expected-blob",
            &blob,
            "--status",
            "done",
            "--orbit-task",
            task,
            "--orbit-run",
            run,
            "--body-file",
            body.to_str().expect("UTF-8 path"),
            "--manifest-file",
            manifest.to_str().expect("UTF-8 path"),
        ]);
        assert_eq!(written["mode"], "worktree");
    }
}

/// One run's step results, in execution order.
struct Run {
    steps: Vec<(String, Result<Value, String>)>,
    worktree: Option<PathBuf>,
}

impl Run {
    fn ids(&self) -> Vec<&str> {
        self.steps.iter().map(|(id, _)| id.as_str()).collect()
    }

    fn failure(&self) -> Option<(&str, &str)> {
        self.steps.iter().find_map(|(id, result)| {
            result
                .as_ref()
                .err()
                .map(|error| (id.as_str(), error.as_str()))
        })
    }
}

/// Orbit's exact-token rendering: a string that is one `{{ path }}` token
/// takes that value's JSON type; objects and arrays render recursively.
fn render(value: &Value, input: &Value, outputs: &BTreeMap<String, Value>) -> Value {
    match value {
        Value::String(text) => {
            let Some(path) = text
                .strip_prefix("{{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .map(str::trim)
            else {
                return value.clone();
            };
            let mut parts = path.split('.');
            let found = match parts.next() {
                Some("input") => parts.try_fold(input, |value, key| value.get(key)),
                Some("steps") => {
                    let step = parts.next().expect("step id");
                    assert_eq!(parts.next(), Some("output"), "{path}");
                    let output = outputs
                        .get(step)
                        .unwrap_or_else(|| panic!("{step} has not run"));
                    parts.try_fold(output, |value, key| value.get(key))
                }
                other => panic!("unsupported template namespace {other:?} in {path}"),
            };
            found
                .cloned()
                .unwrap_or_else(|| panic!("template {path} does not resolve"))
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), render(value, input, outputs)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|value| render(value, input, outputs))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn run_job(fixture: &mut Fixture, agent: Agent) -> Run {
    let job = job();
    let mut input = job["spec"]["default_input"].clone();
    input["task"] = json!(TASK);
    input["base_branch"] = json!(fixture.base_branch());
    let scratch = fixture.temp.path().join("scratch");
    fs::create_dir_all(&scratch).expect("scratch directory");
    let mut outputs = BTreeMap::new();
    let mut run = Run {
        steps: Vec::new(),
        worktree: None,
    };
    for step in job["spec"]["steps"].as_array().expect("steps") {
        let id = step["id"].as_str().expect("step id").to_owned();
        let target = step["target"].as_str().expect("target");
        let name = target.strip_prefix("activity:").expect("activity target");
        let step_input = render(&step["default_input"], &input, &outputs);
        let result = dispatch(fixture, name, &step_input, agent, &scratch, &mut run);
        let failed = result.is_err();
        if let Ok(output) = &result {
            outputs.insert(id.clone(), output.clone());
        }
        run.steps.push((id, result));
        if failed {
            break;
        }
    }
    run
}

fn dispatch(
    fixture: &mut Fixture,
    name: &str,
    input: &Value,
    agent: Agent,
    scratch: &Path,
    run: &mut Run,
) -> Result<Value, String> {
    match name {
        "worktree_setup" => {
            let base = input["base"].as_str().expect("base");
            let worktree = fixture.temp.path().join("run");
            git(
                &fixture.corpus,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("orbit/{TASK}"),
                    worktree.to_str().expect("UTF-8 path"),
                    base,
                ],
            );
            fixture.task_status = "in-progress".into();
            run.worktree = Some(worktree.clone());
            Ok(json!({
                "job_run_id": RUN,
                "workspace_path": worktree.to_string_lossy(),
                "base_ref": base,
                "base_sha": git(&worktree, &["rev-parse", "HEAD"]),
            }))
        }
        "research_investigate" => {
            let spec = &activity(name)["spec"];
            assert_eq!(spec["type"], "agent_loop");
            agent.investigate(scratch, input);
            Ok(json!({"summary": "scripted investigation", "research_id": "R001"}))
        }
        "research_validate" => plugin_tool_call(fixture, name, input),
        "git_commit" => {
            let worktree = Path::new(input["workspace_path"].as_str().expect("workspace_path"));
            assert_eq!(
                git(worktree, &["rev-parse", "HEAD"]),
                input["base_sha"].as_str().expect("base_sha"),
                "HEAD moved before commit"
            );
            git(worktree, &["add", "-A"]);
            git(worktree, &["commit", "-q", "-m", "Deliver R001"]);
            Ok(json!({"committed": true, "commit_sha": git(worktree, &["rev-parse", "HEAD"])}))
        }
        "git_merge" => {
            assert_eq!(input["strategy"], "fast_forward");
            let worktree = Path::new(input["workspace_path"].as_str().expect("workspace_path"));
            let head = git(worktree, &["rev-parse", "HEAD"]);
            git(&fixture.corpus, &["merge", "-q", "--ff-only", &head]);
            Ok(json!({"merged": true}))
        }
        "update_task" => {
            assert_eq!(input["task_id"], TASK);
            fixture.task_status = input["status"].as_str().expect("status").into();
            Ok(json!({}))
        }
        other => panic!("the job targets an activity this harness does not know: {other}"),
    }
}

/// Orbit's deterministic `plugin.tool_call`: the tool and its arguments come
/// from the step input over the activity config, `context.task_id` from the
/// step input's `task_id`, `context.job_run_id` from the run, and
/// `context.workspace_root` is the primary checkout, never the worktree.
fn plugin_tool_call(fixture: &Fixture, name: &str, input: &Value) -> Result<Value, String> {
    let spec = &activity(name)["spec"];
    assert_eq!(spec["type"], "deterministic");
    assert_eq!(spec["action"], "plugin.tool_call");
    let tool = input
        .get("tool")
        .or_else(|| spec["config"].get("tool"))
        .and_then(Value::as_str)
        .expect("tool");
    let args = input
        .get("input")
        .or_else(|| spec["config"].get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let request = json!({
        "schema_version": 1,
        "tool": tool,
        "input": args,
        "context": {
            "workspace_root": fixture.corpus.to_string_lossy(),
            "agent": null,
            "model": null,
            "config": {},
            "task_id": input["task_id"],
            "job_run_id": RUN,
        },
    });
    let mut child = command(BINARY)
        .arg("orbit-tool")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start plugin backend");
    child
        .stdin
        .take()
        .expect("backend stdin")
        .write_all(request.to_string().as_bytes())
        .expect("send request");
    let output = child.wait_with_output().expect("backend reply");
    assert!(output.status.success(), "the backend always exits 0");
    let reply: Value = serde_json::from_slice(&output.stdout).expect("one JSON reply");
    if reply["ok"] == true {
        Ok(reply["output"].clone())
    } else {
        Err(reply["error"]["code"].as_str().unwrap_or("unknown").into())
    }
}

#[test]
fn an_invalid_record_fails_the_run_at_validate_before_commit() {
    for (agent, code) in [
        (Agent::Placeholder, "section_placeholder"),
        (Agent::WrongRun, "orbit_run_mismatch"),
    ] {
        let mut fixture = Fixture::new();
        let base = fixture.head();
        let run = run_job(&mut fixture, agent);
        assert_eq!(run.ids(), ["worktree", "investigate", "validate"]);
        assert_eq!(run.failure(), Some(("validate", code)));
        assert_eq!(fixture.head(), base, "nothing may land on the base branch");
        assert_eq!(fixture.task_status, "in-progress");
        // The worktree keeps the agent's uncommitted files for inspection.
        let worktree = run.worktree.expect("worktree");
        assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), base);
        assert!(!git(&worktree, &["status", "--porcelain"]).is_empty());
    }
}

#[test]
fn a_valid_record_reaches_commit_merges_and_moves_the_task_to_review() {
    let mut fixture = Fixture::new();
    let base = fixture.head();
    let run = run_job(&mut fixture, Agent::Valid);
    assert_eq!(run.failure(), None);
    assert_eq!(
        run.ids(),
        [
            "worktree",
            "investigate",
            "validate",
            "commit",
            "merge",
            "mark_review"
        ]
    );
    let (_, validated) = &run.steps[2];
    let validated = validated.as_ref().expect("validate output");
    assert_eq!(validated["valid"], true);
    assert_eq!(validated["research_id"], "R001");
    assert_eq!(validated["revision"], json!(base));
    assert_eq!(fixture.task_status, "review");

    let corpus = fixture.corpus.to_str().expect("UTF-8 path");
    assert_ne!(fixture.head(), base);
    let record = research(&["research", "show", "--corpus", corpus, "--id", "R001"]);
    assert_eq!(record["metadata"]["status"], "done");
    assert_eq!(
        record["metadata"]["orbit"],
        json!({"task": TASK, "run": RUN})
    );
    // Input bytes stay local: the delivered commit carries only the manifest.
    let delivered = git(
        &fixture.corpus,
        &["show", "--name-only", "--format=", "HEAD"],
    );
    assert_eq!(
        delivered,
        "research/R001-drift/README.md\nresearch/R001-drift/data/manifest.json"
    );
}

#[test]
fn the_job_gates_commit_on_validate_with_no_recovery() {
    let job = job();
    let steps = job["spec"]["steps"].as_array().expect("steps");
    let ids: Vec<&str> = steps.iter().filter_map(|s| s["id"].as_str()).collect();
    assert_eq!(
        ids,
        [
            "worktree",
            "investigate",
            "validate",
            "commit",
            "merge",
            "mark_review"
        ]
    );
    let validate = &steps[2];
    assert_eq!(validate["target"], "activity:research_validate");
    assert!(
        validate.get("recovery_activity").is_none(),
        "a recovery activity would let an invalid record reach commit"
    );
    assert!(job["spec"].get("recovery_activity").is_none());
    assert_eq!(
        validate["default_input"]["input"]["path"],
        "{{ steps.worktree.output.workspace_path }}"
    );
    // `context.task_id` is attested from the step input's own `task_id`.
    assert_eq!(validate["default_input"]["task_id"], "{{ input.task }}");
    assert_eq!(steps[5]["default_input"]["status"], "review");

    for step in steps {
        let name = step["target"]
            .as_str()
            .and_then(|t| t.strip_prefix("activity:"))
            .expect("activity target");
        let own = repo_file(&format!("activities/{name}.yaml")).is_file();
        assert!(
            own || SHIPPED.contains(&name),
            "{name} is neither this plugin's activity nor a shipped one"
        );
        if let Some(recovery) = step["recovery_activity"].as_str() {
            assert!(SHIPPED.contains(&recovery), "{recovery}");
        }
    }
}

#[test]
fn the_validate_activity_calls_the_manifest_read_only_validate_tool() {
    let manifest = yaml("plugin.yaml");
    let spec = &activity("research_validate")["spec"];
    let tool = spec["config"]["tool"].as_str().expect("config.tool");
    // `origin: orbit` registers every tool as `orbit.<name>.<verb>`.
    assert_eq!(manifest["metadata"]["origin"], "orbit");
    let prefix = format!(
        "orbit.{}.",
        manifest["metadata"]["name"].as_str().expect("name")
    );
    let verb = tool.strip_prefix(&prefix).expect("registered tool name");
    let declared = manifest["spec"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == verb)
        .expect("validate is a declared tool");
    assert_eq!(declared["execution_kind"], "read_only");
    // The sandbox constraints this job relies on: no unsandboxed backend and
    // no `requires.programs`, which would fail every deterministic
    // `plugin.tool_call` step. `accept`'s own scratch write root
    // (`.orbit-research-tmp`, never workspace metadata) does not affect this:
    // it is unrelated to `validate`'s read-only step.
    assert_eq!(manifest["spec"]["backend"]["sandbox"], "default");
    assert!(manifest["spec"]["requires"].get("programs").is_none());
}

#[test]
fn the_investigate_prompt_states_the_three_safeguards() {
    let spec = &activity("research_investigate")["spec"];
    let instruction = spec["instruction"].as_str().expect("instruction");
    for safeguard in [
        "Execution success is not support.",
        "Failed controls mean inconclusive.",
        "No H/T edits.",
    ] {
        assert!(
            instruction.contains(safeguard),
            "the prompt must state: {safeguard}"
        );
    }
    let programs = spec["proc_allowed_programs"].as_array().expect("programs");
    assert!(programs.contains(&json!("orbit-research")));
    assert!(instruction.contains("--mode worktree"));
}
