//! The skill works as written. The test extracts the loop's `sh` blocks from
//! `.orbit-plugin/skills/loop/SKILL.md` and runs them, verbatim and in order,
//! on a fresh corpus: init, capture, create the hypothesis, reserve the result,
//! plan. Blocks marked `sh orbit` call Orbit and are not run; the `link` step
//! they lead to is driven here through the real `orbit-tool` backend with the
//! plan's output passed straight through, against a stub `orbit` that is the
//! only `orbit` on PATH. Nothing here can reach a real Orbit.
//!
//! The same shell state then runs the `sh` blocks of the skill's references and
//! of its "Investigate in your own session" section, and the test pins what
//! that section says about `accept` and `assess`. A second test checks that
//! the binary ships exactly the skill directory's files.
#![cfg(unix)]

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SKILL: &str = include_str!("../../../.orbit-plugin/skills/loop/SKILL.md");
const LOOP: &str = "## The loop";
const OWN_SESSION: &str = "## Investigate in your own session";
const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

/// A stub `orbit`: logs each call, keeps one task and answers the two
/// callbacks `link` makes. The input of `orbit.task.add` is kept for the test.
/// `orbit.task.show` answers as a task in `review` that no job ran.
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
  orbit.task.show) echo '{"status":"review","job_run_id":null,"artifacts":[]}' ;;
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

/// The skill directory, read from the checkout.
fn skill_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin/skills/loop")
}

/// The skill's `references/<name>.md` files as (name, text), sorted by name.
fn reference_files() -> Vec<(String, String)> {
    let mut files: Vec<_> = fs::read_dir(skill_directory().join("references"))
        .expect("the references directory")
        .map(|entry| entry.expect("entry").path())
        .map(|path| {
            assert_eq!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("md"),
                "references are Markdown files: {path:?}"
            );
            let name = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            (name, fs::read_to_string(&path).expect("reference text"))
        })
        .collect();
    files.sort();
    files
}

/// The text of the skill's `heading` section, up to the next `## ` heading.
fn section(heading: &str) -> &'static str {
    let start = SKILL
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("no `{heading}` section in the skill"));
    let body = &SKILL[start + 1 + heading.len()..];
    body.find("\n## ").map_or(body, |end| &body[..end])
}

/// The fenced blocks of `info` kind in `text`, in order.
fn fenced_blocks(text: &str, info: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
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
    let blocks = fenced_blocks(section(LOOP), "sh");
    let commands = blocks.concat();
    let order = [
        "workspace init",
        "research capture",
        "--kind H",
        "--kind R --status planned",
        "research plan",
    ]
    .map(|needle| {
        commands
            .find(needle)
            .unwrap_or_else(|| panic!("no `{needle}` command in the skill's loop"))
    });
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    let position = |needle: &str| {
        blocks
            .iter()
            .position(|block| block.contains(needle))
            .unwrap_or_else(|| panic!("no `{needle}` block in the skill"))
    };
    assert!(
        fenced_blocks(section(LOOP), "sh orbit")
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

    // The same shell, continued: the references' `sh` blocks, then the skill's
    // own-session path. The test stands in for the experiment by drafting the
    // body and the manifest that the block's comment asks for.
    let orbit_calls = harness.orbit_calls();
    let mut steps = format!(
        "set -eu\ncorpus='{}'\nscratch='{}'\n",
        corpus.display(),
        scratch[0].display()
    );
    for (_, text) in reference_files() {
        steps.push_str(&fenced_blocks(&text, "sh").concat());
    }
    assert!(
        steps.contains("workspace prepare-operations"),
        "the corpus reference's preparation step is run: {steps}"
    );
    steps.push_str(concat!(
        "printf '## Question\\n\\nDoes it hold?\\n\\n## Method\\n\\nTwo runs and a control.\\n\\n",
        "## Result\\n\\nThe control failed.\\n\\n## Limitations\\n\\nOne host.\\n\\n",
        "## Next\\n\\nRepeat with a working control.\\n' > \"$scratch/body.md\"\n",
        "printf '{\"inputs\": []}\\n' > \"$scratch/manifest.json\"\n",
    ));
    let own_session = fenced_blocks(section(OWN_SESSION), "sh");
    assert!(
        own_session.concat().contains("--mode worktree"),
        "the own-session path writes with the worktree writer"
    );
    steps.push_str(&own_session.concat());
    let output = harness
        .command("/bin/sh")
        .args(["-c", &steps])
        .output()
        .expect("run the own-session steps");
    assert!(
        output.status.success(),
        "the skill's own-session commands failed: {output:?}\n{steps}"
    );
    assert_eq!(
        harness.orbit_calls(),
        orbit_calls,
        "the own-session path never calls Orbit"
    );

    // The result is committed on the corpus's branch and nothing is left over.
    let result = show("R001");
    assert_eq!(result["metadata"]["status"], "done");
    assert_eq!(result["metadata"]["tests"], json!(["H001"]));
    assert!(
        result["body"]
            .as_str()
            .is_some_and(|body| body.contains("The control failed.")),
        "{result}"
    );
    assert_eq!(git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(&["status", "--porcelain", "--untracked-files=all"]), "");
    assert_eq!(
        git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1,
        "the linked worktree was removed"
    );

    // What the skill says about that result: `accept` refuses a task no job
    // ran, and `assess` refuses a result with no acceptance.
    let mut child = harness
        .command(BINARY)
        .arg("orbit-tool")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the plugin backend");
    let request = json!({
        "schema_version": 1,
        "tool": "accept",
        "input": {"task_id": "STUB-1", "research_id": "R001"},
        "context": {"workspace_root": corpus},
    });
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.to_string().as_bytes())
        .expect("send the request");
    let refused = json_of(&child.wait_with_output().expect("reply"));
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no recorded run")),
        "{refused}"
    );
    let hypothesis = show("H001");
    let assess = harness
        .command(BINARY)
        .args(["research", "assess", "--corpus"])
        .arg(&corpus)
        .args(["--id", "H001", "--research", "R001", "--revision", "1"])
        .args(["--verdict", "inconclusive", "--strength", "anecdote"])
        .args([
            "--expected-blob",
            hypothesis["git_blob"].as_str().expect("blob"),
        ])
        .output()
        .expect("assess");
    assert!(!assess.status.success(), "{assess:?}");
    assert_eq!(show("H001")["metadata"]["assessments"], json!([]));
}

#[test]
fn the_binary_ships_exactly_the_skill_directory() {
    // The frontmatter name is the name Orbit links the skill under.
    let frontmatter = SKILL
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(frontmatter, _)| frontmatter)
        .expect("frontmatter");
    let field = |name: &str| {
        frontmatter
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}: ")))
            .unwrap_or_else(|| panic!("no `{name}` in the frontmatter"))
    };
    assert_eq!(field("name"), "research-loop");
    assert!(
        field("description").len() < 1024,
        "the description stays under the provider limit"
    );

    let files = reference_files();
    assert!(!files.is_empty());
    let resource = |arguments: &[&str]| {
        json_of(
            &Command::new(BINARY)
                .args(["--json", "resource"])
                .args(arguments)
                .output()
                .expect("resource"),
        )
    };
    let whole = resource(&[]);
    assert_eq!(whole["skill"], SKILL);
    let shipped = whole["references"].as_object().expect("references");
    assert_eq!(
        shipped.keys().collect::<Vec<_>>(),
        files.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "every file under references/ is embedded, and nothing else"
    );
    for (name, text) in &files {
        assert_eq!(shipped[name], text.as_str(), "{name}");
        let one = resource(&["--reference", name]);
        assert_eq!(one["reference"]["name"], name.as_str());
        assert_eq!(one["reference"]["text"], text.as_str());
        let plain = Command::new(BINARY)
            .args(["resource", "--reference", name])
            .output()
            .expect("resource");
        assert!(plain.status.success(), "{plain:?}");
        assert_eq!(String::from_utf8_lossy(&plain.stdout), text.as_str());
        assert_eq!(
            SKILL.matches(&format!("(references/{name}.md)")).count(),
            1,
            "the skill links `{name}` once"
        );
    }
    assert_eq!(
        SKILL.matches("](references/").count(),
        files.len(),
        "the skill links no reference that does not exist"
    );
}
