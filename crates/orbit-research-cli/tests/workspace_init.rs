use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git should be installed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn ignored(root: &Path, relative: &str) -> bool {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--no-index", "-v", "--", relative])
        .output()
        .expect("git should be installed")
        .stdout;
    let line = String::from_utf8_lossy(&output);
    line.split_once('\t')
        .is_some_and(|(pattern, _)| !pattern.contains(":!"))
}

#[test]
fn generic_workspace_ignores_bytes_but_tracks_manifests() {
    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    let result = workspace::init(root).expect("workspace fixture operation");
    assert_eq!(result["created"], true);
    assert!(root.join("_data").is_dir());

    fs::create_dir_all(root.join("research/R001-run/data")).expect("workspace fixture operation");
    fs::create_dir_all(root.join("research/R001-run/output")).expect("workspace fixture operation");
    fs::create_dir_all(root.join("_data/shared")).expect("workspace fixture operation");
    fs::write(
        root.join("research/R001-run/data/manifest.json"),
        "{\"inputs\":[]}\n",
    )
    .expect("workspace fixture operation");
    fs::write(root.join("research/R001-run/data/raw.bin"), "private\n")
        .expect("workspace fixture operation");
    fs::write(
        root.join("research/R001-run/output/result.bin"),
        "private\n",
    )
    .expect("workspace fixture operation");
    fs::write(root.join("_data/shared/manifest.json"), "{\"inputs\":[]}\n")
        .expect("workspace fixture operation");
    fs::write(root.join("_data/shared/raw.bin"), "private\n").expect("workspace fixture operation");

    assert!(!ignored(root, "research/R001-run/data/manifest.json"));
    assert!(ignored(root, "research/R001-run/data/raw.bin"));
    assert!(ignored(root, "research/R001-run/output/result.bin"));
    assert!(!ignored(root, "_data/shared/manifest.json"));
    assert!(ignored(root, "_data/shared/raw.bin"));

    git(root, &["add", "."]);
    let tracked = git(root, &["ls-files"]);
    assert!(
        tracked
            .lines()
            .any(|path| path == "research/R001-run/data/manifest.json")
    );
    assert!(
        tracked
            .lines()
            .any(|path| path == "_data/shared/manifest.json")
    );
    assert!(!tracked.lines().any(|path| path.ends_with("raw.bin")));
    assert!(!tracked.lines().any(|path| path.ends_with("result.bin")));
}

#[test]
fn existing_corpus_is_validated_without_overwrite() {
    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    workspace::init(root).expect("workspace fixture operation");
    let readme = root.join("README.md");
    let before = fs::read(&readme).expect("workspace fixture operation");
    let first_revision = git(root, &["rev-parse", "HEAD"]);

    let result = workspace::init(root).expect("workspace fixture operation");
    assert_eq!(result["created"], false);
    assert_eq!(result["revision"], first_revision);
    assert_eq!(
        fs::read(&readme).expect("workspace fixture operation"),
        before
    );
}

#[test]
fn invalid_existing_corpus_is_rejected_without_scaffolding() {
    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    fs::create_dir_all(root.join("_scripts")).expect("workspace fixture operation");
    fs::write(
        root.join("_scripts/schema.json"),
        include_bytes!("../../orbit-research-store/resources/schema.json"),
    )
    .expect("workspace fixture operation");
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).expect("workspace fixture operation");
    }
    fs::write(
        root.join("questions/Q001-broken.md"),
        "not a canonical record\n",
    )
    .expect("workspace fixture operation");

    let error = workspace::init(root).expect_err("invalid fixture must refuse");
    assert!(error.contains("invalid-input"));
    assert!(!root.join("README.md").exists());
    assert!(!root.join(".gitignore").exists());
}

#[cfg(unix)]
#[test]
fn unborn_repository_with_schema_is_refused_without_mutation() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    fs::create_dir_all(root.join("_scripts")).expect("workspace fixture operation");
    let schema = b"{\"not\":\"the corpus schema\"}\n";
    fs::write(root.join("_scripts/schema.json"), schema).expect("workspace fixture operation");
    fs::write(root.join("_scripts/check.sh"), "#!/bin/sh\nexit 7\n")
        .expect("workspace fixture operation");
    let mut permissions = fs::metadata(root.join("_scripts/check.sh"))
        .expect("check script metadata")
        .permissions();
    permissions.set_mode(0o640);
    fs::set_permissions(root.join("_scripts/check.sh"), permissions)
        .expect("set fixture permissions");
    git(root, &["init", "-q"]);
    fs::write(root.join("user-notes.txt"), "preserve me\n").expect("workspace fixture operation");

    let before_status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"]);
    let before_schema = fs::read(root.join("_scripts/schema.json")).expect("read fixture schema");
    let before_script = fs::read(root.join("_scripts/check.sh")).expect("read fixture script");
    let before_mode = fs::metadata(root.join("_scripts/check.sh"))
        .expect("check script metadata")
        .permissions()
        .mode();

    let error = workspace::init(root).expect_err("unborn corpus must refuse");
    assert!(error.contains("Corpus has no commits"));
    assert!(error.contains("commit them explicitly or move them aside"));
    assert_eq!(
        git(root, &["status", "--porcelain=v1", "--untracked-files=all"]),
        before_status
    );
    assert_eq!(
        fs::read(root.join("_scripts/schema.json")).expect("read fixture schema"),
        before_schema
    );
    assert_eq!(
        fs::read(root.join("_scripts/check.sh")).expect("read fixture script"),
        before_script
    );
    assert_eq!(
        fs::metadata(root.join("_scripts/check.sh"))
            .expect("check script metadata")
            .permissions()
            .mode(),
        before_mode
    );
    assert_eq!(
        fs::read_to_string(root.join("user-notes.txt")).expect("read user file"),
        "preserve me\n"
    );
}

#[test]
fn failed_initial_commit_can_be_resumed_without_manual_git_repair() {
    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    let failed = workspace::init_without_identity(root);
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("can be resumed"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        fs::read_dir(root)
            .expect("failed scaffold root remains inspectable")
            .next()
            .is_none(),
        "failed scaffold should leave the original empty directory unchanged"
    );

    let result = workspace::init(root).expect("retry should complete scaffold");
    assert_eq!(result["created"], true);
    assert!(!git(root, &["rev-parse", "HEAD"]).is_empty());
}

#[cfg(unix)]
#[test]
fn check_script_is_executable_and_committed_with_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    workspace::init(root).expect("workspace fixture operation");

    let mode = fs::metadata(root.join("_scripts/check.sh"))
        .expect("check script metadata")
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
    assert!(git(root, &["ls-files", "-s", "_scripts/check.sh"]).starts_with("100755 "));
}

// Each subprocess has a fixture identity and no system/global Git config.
// The application itself never invents an author or mutates user configuration.
mod workspace {
    use super::*;

    pub fn init_without_identity(root: &Path) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_orbit-research"))
            .args(["workspace", "init"])
            .arg(root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "user.useConfigOnly")
            .env("GIT_CONFIG_VALUE_0", "true")
            .output()
            .expect("run corpus initialization without identity")
    }

    pub fn init(root: &Path) -> Result<Value, String> {
        let output = Command::new(env!("CARGO_BIN_EXE_orbit-research"))
            .args(["workspace", "init"])
            .arg(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Research fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Research fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .expect("run corpus initialization");
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
    }
}
