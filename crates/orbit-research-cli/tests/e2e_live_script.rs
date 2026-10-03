//! The operator script `scripts/e2e-live.sh` starts a real provider, so its
//! guards are tested here against a stub `orbit` and nothing else: a private
//! HOME, `ORBIT_BIN` naming the stub, and a PATH holding only the stub
//! directory plus the system directories. The harness first proves the real
//! `orbit` cannot be reached, then requires that no refusal ever called the stub.
#![cfg(unix)]

mod exec_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Harness {
    _temp: tempfile::TempDir,
    root: PathBuf,
    stub_dir: PathBuf,
    log: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("private harness");
        let root = temp.path().canonicalize().expect("physical root");
        let stub_dir = root.join("stub-bin");
        fs::create_dir(&stub_dir).expect("stub directory");
        fs::create_dir(root.join("home")).expect("private HOME");
        let log = root.join("stub-calls.log");
        for name in ["orbit", "orbit-research"] {
            let stub = stub_dir.join(name);
            exec_support::install_executable(
                &stub,
                &format!(
                    "#!/bin/sh\nprintf '%s %s\\n' \"{name}\" \"$*\" >> '{}'\nexit 1\n",
                    log.display()
                ),
            );
        }
        let harness = Self {
            _temp: temp,
            root,
            stub_dir,
            log,
        };
        // The real orbit must be unreachable: it resolves to the stub, and the
        // only other PATH entries are the system directories.
        let resolved = Command::new("/bin/sh")
            .args(["-c", "command -v orbit"])
            .env_clear()
            .env("PATH", harness.path())
            .output()
            .expect("resolve orbit");
        assert_eq!(
            String::from_utf8_lossy(&resolved.stdout).trim(),
            harness.stub_dir.join("orbit").to_str().expect("UTF-8 path"),
            "`orbit` must resolve to the stub"
        );
        harness
    }

    fn path(&self) -> String {
        format!("{}:/usr/bin:/bin", self.stub_dir.display())
    }

    fn script(&self, args: &[&str], confirm: bool) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/e2e-live.sh");
        let mut command = Command::new("/bin/sh");
        command
            .arg(script)
            .args(args)
            .env_clear()
            .env("PATH", self.path())
            .env("HOME", self.root.join("home"))
            .env("ORBIT_BIN", self.stub_dir.join("orbit"));
        if confirm {
            command.env("ORBIT_RESEARCH_LIVE_CONFIRM", "yes");
        }
        command.output().expect("run the script")
    }

    fn stub_calls(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn assert_refused(&self, output: &Output, needle: &str) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(stderr.contains(needle), "expected `{needle}` in {stderr}");
        assert_eq!(self.stub_calls(), "", "a refusal must not reach orbit");
    }
}

#[test]
fn nothing_runs_without_live_and_the_confirmation_variable() {
    let harness = Harness::new();
    let corpus = harness.root.join("corpus");
    let corpus = corpus.to_str().expect("UTF-8 path");
    let base = ["--corpus", corpus, "--crew", "sol"];

    for (args, confirm) in [
        (base.to_vec(), false),
        (base.iter().copied().chain(["--live"]).collect(), false),
        (base.to_vec(), true),
    ] {
        let output = harness.script(&args, confirm);
        harness.assert_refused(&output, "dry run only");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Steps (nothing is executed"), "{stdout}");
        assert!(!Path::new(corpus).exists(), "no corpus is created");
    }
}

#[test]
fn plan_and_help_touch_nothing() {
    let harness = Harness::new();
    for args in [&["--plan"][..], &["--help"][..]] {
        let output = harness.script(args, false);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(harness.stub_calls(), "");
    }
}

#[test]
fn plan_reflects_the_given_corpus_crew_and_evidence() {
    let harness = Harness::new();
    let corpus = harness.root.join("fresh-corpus");
    let evidence = harness.root.join("elsewhere/evidence");
    let output = harness.script(
        &[
            "--plan",
            "--corpus",
            corpus.to_str().expect("UTF-8"),
            "--crew",
            "terra",
            "--evidence",
            evidence.to_str().expect("UTF-8"),
            "--workspace",
            "named-workspace",
            "--verdict",
            "refutes",
        ],
        false,
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        corpus.to_str().expect("UTF-8"),
        evidence.to_str().expect("UTF-8"),
        "crew:       terra",
        "named-workspace",
        "refutes",
    ] {
        assert!(
            stdout.contains(expected),
            "expected `{expected}` in {stdout}"
        );
    }
    // Without --workspace and --evidence the defaults derive from the corpus.
    let output = harness.script(
        &[
            "--plan",
            "--corpus",
            corpus.to_str().expect("UTF-8"),
            "--crew",
            "sol",
        ],
        false,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("research-live-fresh-corpus"), "{stdout}");
    assert!(
        stdout.contains(&format!("{}.evidence", corpus.display())),
        "{stdout}"
    );
    assert_eq!(harness.stub_calls(), "");
    assert!(
        !corpus.exists() && !evidence.exists(),
        "--plan creates nothing"
    );
}

#[test]
fn usage_names_live_and_keeps_the_guard_in_the_help_text() {
    let harness = Harness::new();
    let output = harness.script(&["--help"], false);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let usage = stdout.lines().next().expect("usage line");
    assert!(
        usage.contains("--corpus DIR") && usage.contains("--crew NAME") && usage.contains("--live"),
        "{usage}"
    );
    assert!(
        stdout.contains("ORBIT_RESEARCH_LIVE_CONFIRM=yes"),
        "{stdout}"
    );
    assert_eq!(harness.stub_calls(), "");
}

#[test]
fn live_refuses_an_existing_or_nested_corpus_before_any_orbit_call() {
    let harness = Harness::new();

    let taken = harness.root.join("taken");
    fs::create_dir(&taken).expect("existing directory");
    fs::write(taken.join("notes"), "keep").expect("existing file");
    let output = harness.script(
        &[
            "--live",
            "--corpus",
            taken.to_str().expect("UTF-8"),
            "--crew",
            "sol",
        ],
        true,
    );
    harness.assert_refused(&output, "exists and is not empty");
    assert_eq!(
        fs::read_to_string(taken.join("notes")).expect("untouched"),
        "keep"
    );

    let repository = harness.root.join("repo");
    fs::create_dir(&repository).expect("repository directory");
    let init = Command::new("git")
        .args(["init", "-q"])
        .arg(&repository)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git init");
    assert!(init.status.success(), "{init:?}");
    let nested = repository.join("corpus");
    let output = harness.script(
        &[
            "--live",
            "--corpus",
            nested.to_str().expect("UTF-8"),
            "--crew",
            "sol",
        ],
        true,
    );
    harness.assert_refused(&output, "inside the Git work tree");
    assert!(!nested.exists(), "no corpus is created inside a repository");
}

#[test]
fn bad_arguments_are_refused_before_any_orbit_call() {
    let harness = Harness::new();
    for (args, needle) in [
        (vec!["--bogus"], "unknown argument"),
        (
            vec![
                "--live",
                "--corpus",
                "x",
                "--crew",
                "y",
                "--verdict",
                "maybe",
            ],
            "--verdict",
        ),
        (
            vec![
                "--live",
                "--corpus",
                "x",
                "--crew",
                "y",
                "--max-minutes",
                "soon",
            ],
            "--max-minutes",
        ),
    ] {
        let output = harness.script(&args, true);
        assert_ne!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(needle),
            "{args:?}: {output:?}"
        );
        assert_eq!(harness.stub_calls(), "");
    }
}
