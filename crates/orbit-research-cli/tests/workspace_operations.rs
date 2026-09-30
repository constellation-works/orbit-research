use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn isolate(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Research fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("NO_COLOR", "1");
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_orbit-research"));
    isolate(&mut command);
    command
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    isolate(&mut command);
    let output = command
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run fixture Git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git UTF-8")
        .trim()
        .to_owned()
}

fn legacy_corpus() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("private legacy corpus");
    let root = fixture.path();
    for directory in [
        "_scripts",
        "_data",
        "questions",
        "hypotheses",
        "theories",
        "research",
    ] {
        fs::create_dir(root.join(directory)).expect("create corpus directory");
    }
    fs::write(
        root.join("_scripts/schema.json"),
        include_bytes!("../../orbit-research-store/assets/schema.json"),
    )
    .expect("write owner schema");
    fs::write(
        root.join(".gitignore"),
        "_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )
    .expect("write owner ignore policy");
    fs::write(root.join("questions/Q001-preserve.md"), "---\nid: Q001\ntitle: Preserve\nstatus: open\ntags: [evidence]\nderived_from: []\ncreated: 2026-09-01\nupdated: 2026-09-01\nanswered_by: []\n---\nScientific question bytes stay unchanged.\n")
        .expect("write canonical question");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "Legacy corpus"]);
    fixture
}

fn prepare(root: &Path, format: &str) -> Output {
    command()
        .args(["--format", format, "workspace", "prepare-operations"])
        .arg(root)
        .output()
        .expect("run operational preparation")
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("structured preparation output")
}

fn refusal(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error: Value =
        serde_json::from_slice(&output.stderr).expect("structured refusal on stderr");
    error["error"]["message"]
        .as_str()
        .expect("error message")
        .to_owned()
}

#[derive(Debug, PartialEq)]
struct CanonicalState {
    revision: String,
    index: Vec<u8>,
    question: Vec<u8>,
    schema: Vec<u8>,
    ignore: Vec<u8>,
}

fn canonical_state(root: &Path) -> CanonicalState {
    CanonicalState {
        revision: git(root, &["rev-parse", "HEAD"]),
        index: fs::read(root.join(".git/index")).expect("read fixture index"),
        question: fs::read(root.join("questions/Q001-preserve.md")).expect("read question"),
        schema: fs::read(root.join("_scripts/schema.json")).expect("read schema"),
        ignore: fs::read(root.join(".gitignore")).expect("read ignore policy"),
    }
}

#[test]
fn prepare_operations_help_documents_the_explicit_existing_corpus_command() {
    let output = command()
        .args(["workspace", "prepare-operations", "--help"])
        .output()
        .expect("run preparation help");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help UTF-8");
    assert_eq!(
        help,
        include_str!("../src/snapshots/workspace-prepare-operations.help.txt")
    );
    assert!(
        help.contains("workspace prepare-operations [OPTIONS] <PATH>"),
        "{help}"
    );
    assert!(help.contains("existing corpus"), "{help}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn legacy_preparation_is_explicit_and_idempotent_without_scientific_or_git_writes() {
    let fixture = legacy_corpus();
    let root = fixture.path();
    let before = canonical_state(root);
    let validated = success(
        &command()
            .args(["--json", "workspace", "init"])
            .arg(root)
            .output()
            .expect("validate legacy corpus"),
    );
    assert_eq!(validated["created"], false);
    assert_eq!(validated["records"], 1);
    assert!(!root.join("_data/orbit-research-operations").exists());
    assert!(!root.join(".git/orbit-research-operations").exists());
    assert_eq!(canonical_state(root), before);

    let receipt_name = format!("{}.json", "1".repeat(64));
    let receipt = b"\n{\n  \"request_key\": \"legacy-private\",\n  \"result\": {\"id\": \"Q001\", \"mode\": \"primary\"}\n}\n";
    let legacy = root.join(".git/orbit-research-operations");
    fs::create_dir(&legacy).expect("legacy request log");
    fs::write(legacy.join(&receipt_name), receipt).expect("raw legacy receipt");
    fs::write(legacy.join("lock"), "").expect("legacy lock");
    let prepared = success(&prepare(root, "json"));
    assert_eq!(
        prepared["corpus"],
        root.canonicalize()
            .expect("physical corpus")
            .to_str()
            .expect("fixture path")
    );
    assert_eq!(prepared["prepared"], true);
    assert_eq!(prepared["changed"], true);
    assert_eq!(prepared["layout_version"], 1);
    assert_eq!(
        prepared["state_path"],
        root.canonicalize()
            .expect("physical corpus")
            .join("_data/orbit-research-operations")
            .to_str()
            .expect("state path")
    );
    assert!(root.join("_data/orbit-research-operations").is_dir());
    let migrated_receipt = root
        .join("_data/orbit-research-operations")
        .join(&receipt_name);
    assert_eq!(
        fs::read(&migrated_receipt).expect("migrated raw receipt"),
        receipt
    );
    assert_eq!(canonical_state(root), before);

    for format in ["json", "ndjson", "table", "auto"] {
        let output = prepare(root, format);
        assert!(
            output.status.success(),
            "{format}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        if matches!(format, "json" | "ndjson") {
            let result = success(&output);
            assert_eq!(result["prepared"], true);
            assert_eq!(result["changed"], false);
        } else {
            let text = String::from_utf8(output.stdout).expect("human preparation output");
            assert!(text.contains("prepared: true"), "{format}: {text}");
            assert!(text.contains("changed: false"), "{format}: {text}");
        }
        assert_eq!(
            fs::read(&migrated_receipt).expect("unchanged migrated receipt"),
            receipt
        );
        assert_eq!(canonical_state(root), before);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn fresh_initialization_prepares_operations_without_a_second_setup_command() {
    let fixture = tempfile::tempdir().expect("fresh corpus parent");
    let corpus = fixture.path().join("fresh");
    let initialized = success(
        &command()
            .args(["--json", "workspace", "init"])
            .arg(&corpus)
            .output()
            .expect("initialize fresh corpus"),
    );
    assert_eq!(initialized["created"], true);
    assert!(corpus.join("_data/orbit-research-operations").is_dir());
    let revision = git(&corpus, &["rev-parse", "HEAD"]);
    let prepared = success(&prepare(&corpus, "json"));
    assert_eq!(prepared["prepared"], true);
    assert_eq!(prepared["changed"], false);
    assert_eq!(git(&corpus, &["rev-parse", "HEAD"]), revision);
}

#[test]
fn preparation_requires_a_path_and_refuses_missing_or_noncorpus_paths_without_scaffolding() {
    let usage = command()
        .args(["--json", "workspace", "prepare-operations"])
        .output()
        .expect("missing path");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    let error: Value = serde_json::from_slice(&usage.stderr).expect("JSON usage error");
    assert!(
        error["error"]["message"]
            .as_str()
            .expect("usage message")
            .contains("<PATH>")
    );

    let fixture = tempfile::tempdir().expect("refusal fixture");
    let missing = fixture.path().join("missing");
    refusal(&prepare(&missing, "json"));
    assert!(!missing.exists());
    fs::write(fixture.path().join("notes.txt"), "preserve owner notes\n").expect("owner notes");
    refusal(&prepare(fixture.path(), "json"));
    assert_eq!(
        fs::read_to_string(fixture.path().join("notes.txt")).expect("owner notes"),
        "preserve owner notes\n"
    );
    assert_eq!(
        fs::read_dir(fixture.path())
            .expect("unchanged directory")
            .count(),
        1
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn preparation_refuses_unignored_state_and_linked_worktrees_without_canonical_writes() {
    let fixture = legacy_corpus();
    let root = fixture.path();
    fs::write(
        root.join(".gitignore"),
        "# Operational state is not ignored.\n",
    )
    .expect("unsafe ignore policy");
    git(root, &["add", ".gitignore"]);
    git(
        root,
        &["commit", "-q", "-m", "Missing operational ignore policy"],
    );
    let before = canonical_state(root);
    let error = refusal(&prepare(root, "json"));
    assert!(
        error.contains("Operational state must be ignored"),
        "{error}"
    );
    assert_eq!(canonical_state(root), before);
    assert!(!root.join("_data/orbit-research-operations").exists());

    fs::write(
        root.join(".gitignore"),
        "_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )
    .expect("restore ignore policy");
    git(root, &["add", ".gitignore"]);
    git(root, &["commit", "-q", "-m", "Restore owner ignore policy"]);
    let linked_parent = tempfile::tempdir().expect("linked worktree parent");
    let linked = linked_parent.path().join("run");
    git(
        root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "run",
            linked.to_str().expect("worktree path"),
        ],
    );
    let before = canonical_state(root);
    let error = refusal(&prepare(&linked, "json"));
    assert!(error.contains("primary"), "{error}");
    assert!(
        error.contains(
            root.canonicalize()
                .expect("primary root")
                .to_str()
                .expect("primary path")
        ),
        "{error}"
    );
    assert_eq!(canonical_state(root), before);
    assert!(!root.join("_data/orbit-research-operations").exists());
    assert!(!linked.join("_data/orbit-research-operations").exists());
}

#[cfg(unix)]
#[test]
fn preparation_refuses_conflicting_or_symlinked_state_without_writing_its_target() {
    use std::os::unix::fs::symlink;

    let fixture = legacy_corpus();
    let state = fixture.path().join("_data/orbit-research-operations");
    fs::write(&state, "owner file\n").expect("conflicting state file");
    let before = canonical_state(fixture.path());
    refusal(&prepare(fixture.path(), "json"));
    assert_eq!(canonical_state(fixture.path()), before);
    assert_eq!(
        fs::read_to_string(&state).expect("preserved owner file"),
        "owner file\n"
    );
    fs::remove_file(&state).expect("remove private fixture conflict");
    let outside = tempfile::tempdir().expect("private symlink target");
    fs::write(outside.path().join("sentinel"), "untouched\n").expect("private sentinel");
    symlink(outside.path(), &state).expect("unsafe state symlink");
    let before = canonical_state(fixture.path());
    refusal(&prepare(fixture.path(), "json"));
    assert_eq!(canonical_state(fixture.path()), before);
    assert_eq!(
        fs::read_to_string(outside.path().join("sentinel")).expect("read sentinel"),
        "untouched\n"
    );
    assert_eq!(
        fs::read_dir(outside.path())
            .expect("unmodified target")
            .count(),
        1
    );
}
