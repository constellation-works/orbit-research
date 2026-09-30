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
    assert!(include_str!("../workspace.rs").contains("crate::git::command"));
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

    let mut actual = crate::git::read::with_head(root, |view| view.paths())
        .expect("list pinned committed paths");
    actual.sort();
    assert_eq!(actual, expected);
}

#[test]
fn pinned_tree_ignores_head_updates_and_next_snapshot_observes_them() {
    let temp = canonical_fixture();
    let root = temp.path();
    let corpus = Corpus::open(root).expect("open original contract");
    let original = corpus.committed_snapshot().expect("original snapshot");
    let schema = fs::read(root.join("_scripts/schema.json")).expect("original schema bytes");
    let path = "questions/Q001-why.md";
    let original_bytes = fs::read(root.join(path)).expect("original question bytes");
    crate::git::read::with_head(root, |view| {
        assert_eq!(view.revision(), original.revision);
        record(
            root,
            path,
            "id: Q001\ntitle: Why\nstatus: open\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
            "A later question body.",
        );
        record(
            root,
            "questions/Q002-second.md",
            "id: Q002\ntitle: Second\nstatus: open\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nanswered_by: []",
            "Another question.",
        );
        let mut updated_schema: serde_json::Value =
            serde_json::from_slice(&schema).expect("original schema JSON");
        updated_schema["title"] = serde_json::json!("Updated owner contract");
        fs::write(
            root.join("_scripts/schema.json"),
            serde_json::to_vec(&updated_schema).expect("updated schema JSON"),
        )
        .expect("updated owner contract");
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "updated corpus"]);

        assert_eq!(view.bytes(path)?, original_bytes);
        assert_eq!(view.bytes("_scripts/schema.json")?, schema);
        assert!(!view.paths()?.iter().any(|path| path.contains("Q002")));
        Ok(())
    })
    .expect("finish pinned snapshot read");

    let later = corpus
        .committed_snapshot()
        .expect("new snapshot at advanced HEAD");
    assert_ne!(later.revision, original.revision);
    assert_eq!(later.records.len(), 2);
    assert_eq!(later.records[0].body, "A later question body.");
}

#[cfg(unix)]
#[test]
fn committed_snapshot_refuses_symlink_record_entries() {
    let temp = canonical_fixture();
    let root = temp.path();
    std::os::unix::fs::symlink("Q001-why.md", root.join("questions/Q002-escape.md"))
        .expect("committed record symlink");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "symlink record"]);
    let error = Corpus::open(root)
        .expect("open symlink fixture")
        .committed_snapshot()
        .expect_err("committed records must be ordinary files");
    assert!(error.to_string().contains("regular committed files"));
}

/// A hook or another checkout can export these selectors. Explicit corpus
/// writers and scaffolding must never use that foreign repository or index.
#[test]
fn git_environment_cannot_redirect_writes_or_workspace_initialization() {
    const SELECTED: &str = "ORBIT_RESEARCH_SELECTED_GIT_CONTEXT";
    const FRESH: &str = "ORBIT_RESEARCH_FRESH_GIT_CONTEXT";
    if let Some(root) = std::env::var_os(SELECTED) {
        let root = std::path::PathBuf::from(root);
        let corpus = Corpus::open(&root).expect("selected corpus under foreign Git selectors");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let prepared = crate::request_log::prepare_workspace_operations(&root)
                .expect("ignore and indexed-path probes must use the selected owner");
            assert_eq!(prepared["prepared"], true);
            assert_eq!(prepared["changed"], true);
            corpus
                .require_prepared_operations()
                .expect("selected prepared state");
        }
        let reserved = corpus
            .reserve(
                "isolated-git-writer",
                "Q",
                "Selected checkout",
                "Written only to the explicit corpus.",
                vec![],
                vec![],
            )
            .expect("writer must ignore the foreign staged changes");
        assert_eq!(reserved.id, "Q002");
        let snapshot = corpus.committed_snapshot().expect("selected new commit");
        assert_eq!(snapshot.records.len(), 2);
        assert!(snapshot.records.iter().any(|record| record.id == "Q002"));

        let fresh =
            std::path::PathBuf::from(std::env::var_os(FRESH).expect("private scaffold path"));
        let initialized = crate::workspace::init(&fresh)
            .expect("scaffold must initialize its own repository and index");
        assert_eq!(initialized["created"], true);
        let fresh_corpus = Corpus::open(&fresh).expect("new explicit corpus");
        assert!(
            fresh_corpus
                .committed_snapshot()
                .expect("new corpus commit")
                .records
                .is_empty()
        );
        assert!(fresh.join(".git").is_dir());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        fresh_corpus
            .require_prepared_operations()
            .expect("fresh shared state prepared");
        return;
    }

    for mode in ["selectors", "inline-count", "inline-parameters"] {
        let selected = canonical_fixture();
        let foreign = canonical_fixture();
        fs::write(
            selected.path().join(".gitignore"),
            "_data/**\n!_data/**/\n!_data/**/manifest.json\n",
        )
        .expect("selected owner ignore policy");
        git(selected.path(), &["add", ".gitignore"]);
        git(
            selected.path(),
            &["commit", "-q", "-m", "owner ignore policy"],
        );
        let fresh_parent = tempfile::tempdir().expect("private scaffold parent");
        let fresh = fresh_parent.path().join("corpus");
        fs::write(
            foreign.path().join("staged-sentinel"),
            "foreign staged bytes\n",
        )
        .expect("foreign staged sentinel");
        git(foreign.path(), &["add", "staged-sentinel"]);
        fs::write(
            foreign.path().join("untracked-sentinel"),
            "foreign untracked bytes\n",
        )
        .expect("foreign untracked sentinel");
        let foreign_state = foreign.path().join("_data/orbit-research-operations");
        fs::create_dir_all(&foreign_state).expect("foreign indexed state prefix");
        fs::write(
            foreign_state.join("foreign.json"),
            "foreign indexed state\n",
        )
        .expect("foreign indexed state bytes");
        git(foreign.path(), &["add", "_data"]);
        let status = git(foreign.path(), &["status", "--porcelain"]);
        let head = git(foreign.path(), &["rev-parse", "HEAD"]);
        let index = fs::read(foreign.path().join(".git/index")).expect("foreign index bytes");
        let selected_head = git(selected.path(), &["rev-parse", "HEAD"]);
        let original_question =
            fs::read(foreign.path().join("questions/Q001-why.md")).expect("foreign question bytes");
        let before = tree_bytes(foreign.path());
        let mut child = Command::new(std::env::current_exe().expect("test executable"));
        child
            .args([
                "--exact",
                "tests::git::git_environment_cannot_redirect_writes_or_workspace_initialization",
            ])
            .env(SELECTED, selected.path())
            .env(FRESH, &fresh)
            .env("GIT_AUTHOR_NAME", "Isolated writer")
            .env("GIT_AUTHOR_EMAIL", "isolated@example.invalid")
            .env("GIT_COMMITTER_NAME", "Isolated writer")
            .env("GIT_COMMITTER_EMAIL", "isolated@example.invalid");
        for name in [
            "GIT_DIR",
            "GIT_COMMON_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            child.env_remove(name);
        }
        match mode {
            "selectors" => {
                child
                    .env("GIT_DIR", foreign.path().join(".git"))
                    .env("GIT_COMMON_DIR", foreign.path().join(".git"))
                    .env("GIT_WORK_TREE", foreign.path())
                    .env("GIT_INDEX_FILE", foreign.path().join(".git/index"))
                    .env("GIT_OBJECT_DIRECTORY", foreign.path().join(".git/objects"))
                    .env(
                        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                        foreign.path().join(".git/objects"),
                    );
            }
            "inline-count" => {
                child
                    .env("GIT_CONFIG_COUNT", "1")
                    .env("GIT_CONFIG_KEY_0", "core.worktree")
                    .env("GIT_CONFIG_VALUE_0", foreign.path());
            }
            "inline-parameters" => {
                child.env(
                    "GIT_CONFIG_PARAMETERS",
                    format!("'core.worktree={}'", foreign.path().display()),
                );
            }
            _ => unreachable!("fixed fixture mode"),
        }
        let output = child
            .output()
            .expect("re-exec under hostile Git repository selectors");
        assert!(
            output.status.success(),
            "explicit corpus operation used foreign Git context ({mode}):\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_ne!(git(selected.path(), &["rev-parse", "HEAD"]), selected_head);
        assert_eq!(git(foreign.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read(foreign.path().join(".git/index")).expect("preserved foreign index"),
            index
        );
        assert_eq!(git(foreign.path(), &["status", "--porcelain"]), status);
        assert_eq!(
            fs::read(foreign.path().join("questions/Q001-why.md"))
                .expect("preserved foreign question"),
            original_question
        );
        assert_eq!(
            fs::read(foreign.path().join("staged-sentinel")).expect("preserved staged sentinel"),
            b"foreign staged bytes\n"
        );
        assert_eq!(
            fs::read(foreign.path().join("untracked-sentinel"))
                .expect("preserved untracked sentinel"),
            b"foreign untracked bytes\n"
        );
        assert!(
            !foreign
                .path()
                .join("questions/Q002-selected-checkout.md")
                .exists()
        );
        assert!(fresh.join(".git/index").is_file());
        assert_eq!(
            tree_bytes(foreign.path()),
            before,
            "foreign config, refs, objects, index and files must remain exact ({mode})"
        );
    }
}

fn tree_bytes(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn collect(root: &Path, result: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(root).expect("private foreign repository entries") {
            let path = entry.expect("private repository entry").path();
            if path.is_dir() {
                collect(&path, result);
            } else {
                result.insert(
                    path.clone(),
                    fs::read(path).expect("private repository file bytes"),
                );
            }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    collect(root, &mut result);
    result
}
