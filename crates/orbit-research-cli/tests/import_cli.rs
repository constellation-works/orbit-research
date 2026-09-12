//! CLI-level coverage for `orbit-research import`: with and without
//! `--dry-run` the source root is never mutated, and reports only ever land
//! on stdout or at a new `--output` path outside the source root.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_orbit-research")
}

fn run(args: &[&str]) -> Output {
    Command::new(binary()).args(args).output().expect("spawn orbit-research")
}

fn write_fixture(root: &Path) {
    let gates = root.join("gates");
    fs::create_dir_all(&gates).expect("mkdir gates");
    fs::write(
        gates.join("control.json"),
        r#"{"id":"control","control":"zero injected effect","kill":"absolute error >= 0.04","decision":"Failed control leaves primary inference unresolved."}"#,
    )
    .expect("write control.json");
}

fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walk(dir: &Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
    for entry in fs::read_dir(dir).expect("read_dir").filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push((path.clone(), fs::read(&path).expect("read")));
        }
    }
}

#[test]
fn dry_run_flag_present_or_absent_never_writes_scientific_records() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("principia");
    fs::create_dir(&root).expect("mkdir root");
    write_fixture(&root);
    let before = snapshot(&root);

    for extra in [vec!["--dry-run"], vec![]] {
        let mut args = vec!["import", "principia", "--source-root", root.to_str().expect("utf8"), "--repository", "principia"];
        args.extend(extra);
        let output = run(&args);
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("stdout is a JSON report");
        assert_eq!(report["kind"], "import-report");
        assert_eq!(report["dry_run"], true);

        let after = snapshot(&root);
        assert_eq!(after, before, "import must not create, delete or mutate files in the source root");
    }
}

#[test]
fn output_flag_writes_outside_source_root_and_refuses_unsafe_destinations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("principia");
    fs::create_dir(&root).expect("mkdir root");
    write_fixture(&root);

    // Refuses a destination inside the source root.
    let inside = root.join("report.json");
    let output = run(&[
        "import",
        "principia",
        "--source-root",
        root.to_str().expect("utf8"),
        "--repository",
        "principia",
        "--output",
        inside.to_str().expect("utf8"),
    ]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(!inside.exists());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).expect("stderr is JSON");
    assert_eq!(error["error"]["code"], "invalid-input");

    // Succeeds outside the source root; the summary carries counts, not the full report.
    let outside = tmp.path().join("report.json");
    let output = run(&[
        "import",
        "principia",
        "--source-root",
        root.to_str().expect("utf8"),
        "--repository",
        "principia",
        "--output",
        outside.to_str().expect("utf8"),
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(outside.exists());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(summary["source_unchanged"], true);
    assert!(summary.get("counts").is_some());

    // Refuses to overwrite the file it just wrote.
    let output = run(&[
        "import",
        "principia",
        "--source-root",
        root.to_str().expect("utf8"),
        "--repository",
        "principia",
        "--output",
        outside.to_str().expect("utf8"),
    ]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}
