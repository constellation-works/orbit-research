use crate::{Error, workspace};
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
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let result = workspace::init(root).unwrap();
    assert_eq!(result["created"], true);
    assert!(root.join("_data").is_dir());

    fs::create_dir_all(root.join("research/R001-run/data")).unwrap();
    fs::create_dir_all(root.join("research/R001-run/output")).unwrap();
    fs::create_dir_all(root.join("_data/shared")).unwrap();
    fs::write(
        root.join("research/R001-run/data/manifest.json"),
        "{\"inputs\":[]}\n",
    )
    .unwrap();
    fs::write(root.join("research/R001-run/data/raw.bin"), "private\n").unwrap();
    fs::write(
        root.join("research/R001-run/output/result.bin"),
        "private\n",
    )
    .unwrap();
    fs::write(root.join("_data/shared/manifest.json"), "{\"inputs\":[]}\n").unwrap();
    fs::write(root.join("_data/shared/raw.bin"), "private\n").unwrap();

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
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workspace::init(root).unwrap();
    let readme = root.join("README.md");
    let before = fs::read(&readme).unwrap();
    let first_revision = git(root, &["rev-parse", "HEAD"]);

    let result = workspace::init(root).unwrap();
    assert_eq!(result["created"], false);
    assert_eq!(result["revision"], first_revision);
    assert_eq!(fs::read(&readme).unwrap(), before);
}

#[test]
fn invalid_existing_corpus_is_rejected_without_scaffolding() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("_scripts")).unwrap();
    fs::write(
        root.join("_scripts/schema.json"),
        include_bytes!("../../resources/schema.json"),
    )
    .unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    fs::write(
        root.join("questions/Q001-broken.md"),
        "not a canonical record\n",
    )
    .unwrap();

    let error = workspace::init(root).unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));
    assert!(!root.join("README.md").exists());
    assert!(!root.join(".gitignore").exists());
}
