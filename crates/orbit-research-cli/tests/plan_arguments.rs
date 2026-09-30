use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Planning fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Planning fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn run(args: &[&str]) -> Output {
    command().args(args).output().expect("run orbit-research")
}

fn corpus() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let root = temp.path().join("corpus");
    let path = root.to_str().expect("UTF-8 fixture path");
    for args in [
        vec!["workspace", "init", path],
        vec![
            "research",
            "create",
            "--corpus",
            path,
            "--kind",
            "R",
            "--title",
            "Plans",
            "--request-key",
            "plan-fixture",
        ],
    ] {
        let output = run(&args);
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    temp
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
fn missing_shape_arguments_are_usage_errors_before_opening_a_corpus() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let root = temp.path().join("never-created");
    for (shape, supplied, missing) in [
        ("investigation", vec![], "--objective"),
        ("contribution", vec!["--unit", "test"], "--objective"),
        ("contribution", vec!["--objective", "test"], "--unit"),
        ("synthesis", vec![], "--contribution"),
    ] {
        for format in ["auto", "table", "json", "ndjson"] {
            let mut args = vec![
                "--format",
                format,
                "research",
                "plan",
                "--corpus",
                root.to_str().expect("UTF-8 fixture path"),
                "--research-id",
                "R001",
                "--shape",
                shape,
            ];
            args.extend(&supplied);
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            let error = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
            assert!(error.contains(missing), "{error}");
            if matches!(format, "json" | "ndjson") {
                let error: Value = serde_json::from_str(&error).expect("machine error");
                assert_eq!(error["error"]["code"], "invalid-input");
            }
        }
    }
    assert!(!root.exists());
}

#[test]
fn planning_rejects_ignored_flags_and_valid_shapes_remain_read_only() {
    let temp = corpus();
    let root = temp.path().join("corpus");
    let path = root.to_str().expect("UTF-8 fixture path");
    let head = git(&root, &["rev-parse", "HEAD"]);
    for supplied in [
        vec![
            "--shape",
            "investigation",
            "--objective",
            "Test",
            "--unit",
            "ignored",
        ],
        vec![
            "--shape",
            "investigation",
            "--objective",
            "Test",
            "--contribution",
            "ignored",
        ],
        vec![
            "--shape",
            "contribution",
            "--objective",
            "Test",
            "--unit",
            "work",
            "--contribution",
            "ignored",
        ],
        vec![
            "--shape",
            "synthesis",
            "--contribution",
            "work",
            "--objective",
            "ignored",
        ],
        vec![
            "--shape",
            "synthesis",
            "--contribution",
            "work",
            "--unit",
            "ignored",
        ],
    ] {
        let mut args = vec![
            "--json",
            "research",
            "plan",
            "--corpus",
            path,
            "--research-id",
            "R001",
        ];
        args.extend(supplied);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let error: Value = serde_json::from_slice(&output.stderr).expect("machine error");
        assert_eq!(error["error"]["code"], "invalid-input");
    }
    for (supplied, expected) in [
        (
            vec!["--shape", "investigation", "--objective", "Test"],
            "Investigate R001",
        ),
        (
            vec![
                "--shape",
                "contribution",
                "--objective",
                "Test",
                "--unit",
                "work",
            ],
            "Contribute work to R001",
        ),
        (
            vec!["--shape", "synthesis", "--contribution", "work"],
            "Synthesize R001 from work",
        ),
    ] {
        let mut args = vec![
            "--json",
            "research",
            "plan",
            "--corpus",
            path,
            "--research-id",
            "R001",
        ];
        args.extend(supplied);
        let output = run(&args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let draft: Value = serde_json::from_slice(&output.stdout).expect("task draft");
        assert_eq!(draft["title"], expected);
        assert!(
            !draft["context_files"]
                .as_array()
                .expect("context files")
                .is_empty()
        );
    }
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
}
