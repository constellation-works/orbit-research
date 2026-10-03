//! The skill's loop works as written. The test extracts the loop's `sh` blocks
//! from `.orbit-plugin/skills/native/SKILL.md` and runs them, verbatim and in
//! order, on a fresh corpus: init, capture, create the hypothesis, reserve the
//! result, plan. Blocks marked `sh orbit` call Orbit and are not run; the `link`
//! step they lead to is driven here through the real `orbit-tool` backend with
//! the plan's output passed straight through, against a stub `orbit` that is
//! the only `orbit` on PATH. Nothing here can reach a real Orbit.
#![cfg(unix)]

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SKILL: &str = include_str!("../../../.orbit-plugin/skills/native/SKILL.md");
const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

/// A stub `orbit`: logs each call, keeps one task and answers the two
/// callbacks `link` makes. The input of `orbit.task.add` is kept for the test.
const STUB_ORBIT: &str = r#"#!/bin/sh
dir=$(dirname "$0")
printf '%s\n' "$*" >> "$dir/calls.log"
case "$3" in
  orbit.task.list)
    if [ -f "$dir/task" ]; then printf '{"tasks":[{"id":"%s"}]}\n' "$(cat "$dir/task")"; else echo '{"tasks":[]}'; fi ;;
  orbit.task.add)
    printf '%s' "$5" > "$dir/added.json"
    echo STUB-1 > "$dir/task"
    echo '{"id":"STUB-1"}' ;;
  *) echo "unexpected orbit call: $*" >&2; exit 1 ;;
esac
"#;

struct Harness {
    _temp: tempfile::TempDir,
    root: PathBuf,
    work: PathBuf,
    stub: PathBuf,
    /// The `TMPDIR` the skill's `mktemp -d` scratch directory is made in.
    tmp: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("private harness");
        let root = temp.path().canonicalize().expect("physical root");
        let work = root.join("work");
        let stub = root.join("stub-bin");
        let tmp = root.join("tmp");
        for directory in [&work, &stub, &tmp, &root.join("home")] {
            fs::create_dir(directory).expect("harness directory");
        }
        symlink(BINARY, stub.join("orbit-research")).expect("orbit-research on PATH");
        let orbit = stub.join("orbit");
        fs::write(&orbit, STUB_ORBIT).expect("stub orbit");
        fs::set_permissions(&orbit, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        Self {
            _temp: temp,
            root,
            work,
            stub,
            tmp,
        }
    }

    /// A command with no ambient Git configuration or identity, and a PATH of
    /// the stub directory plus the system directories.
    fn command(&self, program: &str) -> Command {
        let null = "/dev/null";
        let mut command = Command::new(program);
        command
            .current_dir(&self.work)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.stub.display()))
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", &self.tmp)
            .env("ORBIT_BIN", self.stub.join("orbit"))
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_CONFIG_SYSTEM", null)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Walkthrough")
            .env("GIT_AUTHOR_EMAIL", "walkthrough@example.invalid")
            .env("GIT_COMMITTER_NAME", "Walkthrough")
            .env("GIT_COMMITTER_EMAIL", "walkthrough@example.invalid");
        command
    }

    fn orbit_calls(&self) -> String {
        fs::read_to_string(self.stub.join("calls.log")).unwrap_or_default()
    }
}

/// The fenced blocks of `info` kind in the skill's loop section, in order.
fn loop_blocks(info: &str) -> Vec<String> {
    let start = SKILL.find("## The loop").expect("the loop section");
    let end = SKILL
        .find("## Two writer modes")
        .expect("the end of the loop");
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in SKILL[start..end].lines() {
        match (&mut current, line.strip_prefix("```")) {
            (None, Some(opening)) if opening == info => current = Some(String::new()),
            (Some(block), Some("")) => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            (Some(block), _) => {
                block.push_str(line);
                block.push('\n');
            }
            _ => {}
        }
    }
    assert!(current.is_none(), "unterminated fence");
    blocks
}

fn json_of(output: &std::process::Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

#[test]
fn the_skills_loop_reaches_plan_and_link_as_written() {
    let harness = Harness::new();

    // The documented order: the result is reserved before it is planned.
    let blocks = loop_blocks("sh");
    let position = |needle: &str| {
        blocks
            .iter()
            .position(|block| block.contains(needle))
            .unwrap_or_else(|| panic!("no `{needle}` block in the skill"))
    };
    let order = [
        position("workspace init"),
        position("research capture"),
        position("--kind H"),
        position("--kind R --status planned"),
        position("research plan"),
    ];
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    assert!(
        loop_blocks("sh orbit")
            .iter()
            .any(|block| block.contains("orbit.research.link")),
        "the loop's link step is documented"
    );

    // Follow the non-Orbit steps literally in one shell. The plan step runs
    // with the corpus as the working directory, the worst case for a plan
    // written into "the current directory": the skill must keep it out.
    let plan_step = position("research plan");
    let mut steps = String::new();
    for (index, block) in blocks.iter().enumerate() {
        if index == plan_step {
            steps.push_str("cd \"$corpus\"\n");
        }
        steps.push_str(block);
    }
    let script = format!("set -eu\n{steps}");
    let output = harness
        .command("/bin/sh")
        .args(["-c", &script])
        .output()
        .expect("run the skill's steps");
    assert!(
        output.status.success(),
        "the skill's own commands failed: {output:?}\n{script}"
    );
    assert_eq!(harness.orbit_calls(), "", "these steps never call Orbit");

    // The corpus holds exactly what the skill says it creates.
    let corpus = harness.work.join("research-corpus");
    let git = |args: &[&str]| {
        let output = harness
            .command("git")
            .arg("-C")
            .arg(&corpus)
            .args(args)
            .output()
            .expect("git");
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout)
            .expect("UTF-8")
            .trim()
            .to_owned()
    };
    assert_eq!(git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(
        git(&["status", "--porcelain", "--untracked-files=all"]),
        "",
        "every write committed and the plan step left the corpus clean"
    );
    assert!(
        !corpus.join("plan.json").exists() && !harness.work.join("plan.json").exists(),
        "the plan is written outside the corpus and the working directory"
    );
    let show = |id: &str| {
        json_of(
            &harness
                .command(BINARY)
                .args(["--json", "research", "show", "--corpus"])
                .arg(&corpus)
                .args(["--id", id])
                .output()
                .expect("show"),
        )
    };
    let hypothesis = show("H001");
    assert_eq!(hypothesis["metadata"]["status"], "open");
    assert_eq!(hypothesis["metadata"]["derived_from"], json!(["Q001"]));
    assert!(
        hypothesis["git_blob"]
            .as_str()
            .is_some_and(|b| !b.is_empty())
    );
    assert_eq!(hypothesis["metadata"]["revision"], 1);
    let result = show("R001");
    assert_eq!(result["metadata"]["status"], "planned");
    assert_eq!(result["metadata"]["derived_from"], json!(["Q001", "H001"]));

    // `plan`'s output goes straight to `link`, plus the two ids the loop adds.
    let scratch: Vec<_> = fs::read_dir(&harness.tmp)
        .expect("TMPDIR")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(scratch.len(), 1, "one scratch directory: {scratch:?}");
    let plan: Value =
        serde_json::from_slice(&fs::read(scratch[0].join("plan.json")).expect("plan.json"))
            .expect("plan JSON");
    let scope = plan["context_files"].clone();
    assert_eq!(
        scope,
        json!(["dir:research/R001-load-test-of-the-baseline"])
    );
    let mut input = plan;
    input["research_id"] = json!("R001");
    input["request_key"] = json!("link-R001");
    let link = |input: &Value| {
        let mut child = harness
            .command(BINARY)
            .arg("orbit-tool")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the plugin backend");
        let request = json!({
            "schema_version": 1,
            "tool": "link",
            "input": input,
            "context": {"workspace_root": corpus},
        });
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(request.to_string().as_bytes())
            .expect("send the request");
        json_of(&child.wait_with_output().expect("reply"))
    };

    let mut widened = input.clone();
    widened["context_files"] = json!([
        "dir:research/R001-load-test-of-the-baseline",
        "dir:questions"
    ]);
    let refused = link(&widened);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("context_files")),
        "{refused}"
    );
    assert_eq!(
        harness.orbit_calls(),
        "",
        "a refused scope never reaches Orbit"
    );

    let linked = link(&input);
    assert_eq!(linked["ok"], true, "{linked}");
    assert_eq!(linked["output"]["created"], true);
    assert_eq!(linked["output"]["task_id"], "STUB-1");
    let added: Value =
        serde_json::from_slice(&fs::read(harness.stub.join("added.json")).expect("task input"))
            .expect("orbit.task.add input");
    assert_eq!(added["context_files"], scope);
    assert_eq!(added["title"], input["title"]);
    assert_eq!(added["acceptance_criteria"], input["acceptance_criteria"]);
    assert!(
        Path::new(&corpus)
            .join("_data/orbit-research-operations")
            .is_dir()
    );
}
