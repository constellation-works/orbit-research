use crate::corpus::Corpus;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");
/// Re-exec marker: when set, this process is the PATH-cleared child spawned by
/// `read_and_validate_paths_do_not_spawn_a_process` below, not the top-level test run.
const REEXEC_ENV: &str = "ORBIT_RESEARCH_GIT_FREE_CHILD";
const REEXEC_ROOT_ENV: &str = "ORBIT_RESEARCH_GIT_FREE_CHILD_ROOT";

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

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn record(root: &Path, relative: &str, frontmatter: &str, body: &str) {
    write(root, relative, &format!("---\n{frontmatter}\n---\n{body}"));
}

fn canonical_fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    record(
        root,
        "questions/Q001-why.md",
        "id: Q001\ntitle: Why\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
        "Question body.",
    );
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
    git(root, &["config", "user.name", "Git read tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    temp
}

/// The reading modules never spawn a process: neither `git/read.rs`, nor the
/// read/validate call sites in `corpus.rs`, mention `Command::new`. This is a
/// structural companion to the PATH-emptied behavioral test below; either
/// alone would already prove the property, together they are cheap insurance
/// against drift in whichever one a future change forgets to update.
#[test]
fn read_and_validate_modules_never_mention_process_spawning() {
    for (path, source) in [
        ("git/read.rs", include_str!("../git/read.rs") as &str),
        ("corpus.rs", include_str!("../corpus.rs")),
        ("record.rs", include_str!("../record.rs")),
    ] {
        assert!(
            !source.contains("Command::new"),
            "{path} must stay process-free for the read/validate path, but mentions Command::new"
        );
    }
    // The writer's escape hatch is expected to still spawn `git`; confirm the
    // split actually exists rather than the read side accidentally losing all coverage.
    assert!(include_str!("../git/mod.rs").contains("Command::new"));
    assert!(include_str!("../workspace.rs").contains("Command::new"));
}

/// End-to-end proof, not just textual: every read/validate operation succeeds
/// with `PATH` cleared, so nothing on this path can be shelling out to `git`.
/// Runs in a re-exec'd child so clearing `PATH` cannot starve the sibling
/// tests in this binary that legitimately spawn `git` to build fixtures.
#[test]
fn read_and_validate_paths_do_not_spawn_a_process() {
    if let Ok(root) = std::env::var(REEXEC_ROOT_ENV) {
        assert!(std::env::var(REEXEC_ENV).is_ok());
        exercise_read_and_validate_paths(Path::new(&root));
        return;
    }

    let temp = canonical_fixture();
    let exe = std::env::current_exe().expect("current test binary path");
    let output = Command::new(exe)
        .arg("--exact")
        .arg("tests::git::read_and_validate_paths_do_not_spawn_a_process")
        .env(REEXEC_ENV, "1")
        .env(REEXEC_ROOT_ENV, temp.path())
        .env_remove("PATH")
        .output()
        .expect("re-exec the test binary with PATH cleared");
    assert!(
        output.status.success(),
        "read/validate path spawned a process once PATH was cleared:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn exercise_read_and_validate_paths(root: &Path) {
    let corpus = Corpus::open(root).expect("open corpus without PATH");
    let snapshot = corpus.snapshot().expect("snapshot without PATH");
    let committed = corpus
        .committed_snapshot()
        .expect("committed_snapshot without PATH");
    assert_eq!(snapshot.revision, committed.revision);

    let record = committed
        .records
        .iter()
        .find(|record| record.id == "Q001")
        .expect("fixture record");
    let blob = corpus
        .committed_blob(&committed.revision, &record.path)
        .expect("committed_blob without PATH");
    assert_eq!(blob, record.git_blob);
    let bytes = corpus
        .committed_bytes(&committed.revision, &record.path)
        .expect("committed_bytes without PATH");
    assert!(!bytes.is_empty());
    assert!(
        corpus
            .published(&committed.revision, &committed.revision)
            .expect("published without PATH")
    );
}

#[test]
fn resolves_head_through_packed_refs() {
    let temp = canonical_fixture();
    let root = temp.path();
    let expected = git(root, &["rev-parse", "HEAD"]);

    git(root, &["pack-refs", "--all", "--prune"]);
    assert!(
        root.join(".git/packed-refs").exists(),
        "fixture must actually exercise packed refs"
    );

    let snapshot = Corpus::open(root).unwrap().snapshot().unwrap();
    assert_eq!(snapshot.revision, expected);
}

#[test]
fn resolves_a_detached_head() {
    let temp = canonical_fixture();
    let root = temp.path();
    let expected = git(root, &["rev-parse", "HEAD"]);

    git(root, &["checkout", "--detach", "-q", &expected]);
    let head_contents = fs::read_to_string(root.join(".git/HEAD")).unwrap();
    assert!(
        !head_contents.trim_start().starts_with("ref:"),
        "fixture must actually exercise a detached HEAD: {head_contents}"
    );

    let snapshot = Corpus::open(root).unwrap().snapshot().unwrap();
    assert_eq!(snapshot.revision, expected);
    let committed = Corpus::open(root).unwrap().committed_snapshot().unwrap();
    assert_eq!(committed.revision, expected);
}

#[test]
fn committed_paths_matches_ls_tree_recursive_listing() {
    let temp = canonical_fixture();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let mut expected: Vec<String> = git(root, &["ls-tree", "-r", "--name-only", &head])
        .lines()
        .map(str::to_owned)
        .collect();
    expected.sort();

    let corpus = Corpus::open(root).unwrap();
    let mut actual = corpus.committed_paths(&head).unwrap();
    actual.sort();
    assert_eq!(actual, expected);
}
