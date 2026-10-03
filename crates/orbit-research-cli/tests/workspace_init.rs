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
    // First run and re-run report the same fields.
    assert_eq!(result["records"], 0);
    assert_eq!(
        result
            .as_object()
            .expect("receipt")
            .keys()
            .collect::<Vec<_>>(),
        workspace::init(root)
            .expect("re-run")
            .as_object()
            .expect("receipt")
            .keys()
            .collect::<Vec<_>>()
    );
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
    assert_eq!(result["records"], 0);
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
        include_bytes!("../../orbit-research-store/assets/schema.json"),
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
fn a_missing_git_identity_is_summarized_and_leaves_nothing_behind() {
    let temp = tempfile::tempdir().expect("workspace fixture operation");
    let root = temp.path();
    let failed = workspace::init_without_identity(root);
    assert!(!failed.status.success());
    let error: Value = serde_json::from_slice(&failed.stderr).expect("structured error");
    let stderr = error["error"]["message"].as_str().expect("message");
    // The refusal says what is true (nothing was created) and gives the exact
    // commands, instead of Git's multi-line "Please tell me who you are".
    assert!(stderr.contains("nothing was created"), "{stderr}");
    assert!(
        stderr.contains("git config --global user.name \"Your Name\""),
        "{stderr}"
    );
    assert!(
        stderr.contains("git config --global user.email you@example.com"),
        "{stderr}"
    );
    assert!(
        stderr.contains("rerun `orbit-research workspace init "),
        "{stderr}"
    );
    for raw in [
        "Please tell me who you are",
        "Scaffold is incomplete",
        "can be resumed",
    ] {
        assert!(!stderr.contains(raw), "{raw}: {stderr}");
    }
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

/// A fresh corpus is on `main` whatever Git's own default branch says: the
/// `research_investigation` job's `base_branch` defaults to `main`.
#[test]
fn fresh_corpus_is_on_main_regardless_of_git_configuration() {
    let fixture = tempfile::tempdir().expect("private fixture");
    let trunk = fixture.path().join("trunk.gitconfig");
    fs::write(&trunk, "[init]\n\tdefaultBranch = trunk\n").expect("write Git config");
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    for (name, global) in [("null", Path::new(null)), ("trunk", trunk.as_path())] {
        let corpus = fixture.path().join(name);
        let output = Command::new(env!("CARGO_BIN_EXE_orbit-research"))
            .args(["--format", "json", "workspace", "init"])
            .arg(&corpus)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_GLOBAL", global)
            .env("GIT_CONFIG_SYSTEM", null)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Research fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Research fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .expect("run corpus initialization");
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(git(&corpus, &["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(git(&corpus, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    }
}

// Each subprocess has a fixture identity and no system/global Git config.
// The application itself never invents an author or mutates user configuration.
mod workspace {
    use super::*;

    pub fn init_without_identity(root: &Path) -> std::process::Output {
        // The Git boundary clears command-scoped config overrides along with
        // repository redirects. Keep this identity requirement in an isolated
        // configuration file so the fixture still proves commit-failure rollback.
        let config = tempfile::NamedTempFile::new().expect("private strict Git config");
        fs::write(config.path(), "[user]\n\tuseConfigOnly = true\n")
            .expect("require an explicit fixture identity");
        Command::new(env!("CARGO_BIN_EXE_orbit-research"))
            .args(["--format", "json", "workspace", "init"])
            .arg(root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_GLOBAL", config.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run corpus initialization without identity")
    }

    pub fn init(root: &Path) -> Result<Value, String> {
        let output = Command::new(env!("CARGO_BIN_EXE_orbit-research"))
            .args(["--format", "json", "workspace", "init"])
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
