use orbit_research_core::{Research as Corpus, work::WorkMode};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn corpus() -> (TempDir, Corpus) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
    git(root, &["config", "user.name", "Work tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    let corpus = Corpus::open(root).unwrap();
    corpus
        .reserve("r1", "R", "Study", "Question", vec!["lab".into()], vec![])
        .unwrap();
    (temp, corpus)
}

#[test]
fn contribution_owns_disjoint_unit_paths() {
    let (_temp, corpus) = corpus();
    let plan = corpus
        .contribution("R001", "control-a", "Measure the control.")
        .unwrap();
    assert_eq!(plan.mode, WorkMode::Contribution);
    assert_eq!(
        plan.context_files,
        vec![
            "dir:research/R001-study/code/control-a",
            "dir:research/R001-study/artifacts/control-a",
        ]
    );
    assert!(
        !plan
            .context_files
            .iter()
            .any(|path| path.contains("README") || path.contains("manifest"))
    );
    assert!(
        plan.instructions
            .contains("do not edit it or the shared data/manifest.json")
    );
    assert!(plan.instructions.contains("findings.md"));
}

#[test]
fn synthesis_scopes_shared_summary_and_manifest() {
    let (_temp, corpus) = corpus();
    let plan = corpus
        .synthesis("R001", &["control-a".into(), "control-b".into()])
        .unwrap();
    assert_eq!(plan.mode, WorkMode::Synthesis);
    assert_eq!(
        plan.context_files,
        vec![
            "file:research/R001-study/README.md",
            "file:research/R001-study/data/manifest.json",
        ]
    );
    assert!(plan.instructions.contains("research/R001-study/README.md"));
    assert!(
        plan.instructions
            .contains("research/R001-study/artifacts/control-a/findings.md")
    );
    assert!(
        plan.instructions
            .contains("research/R001-study/artifacts/control-b/findings.md")
    );
}

#[test]
fn work_rejects_traversal_missing_research_and_incomplete_synthesis() {
    let (_temp, corpus) = corpus();
    assert!(
        corpus
            .contribution("R001", "../escape", "objective")
            .is_err()
    );
    assert!(corpus.contribution("Q001", "unit", "objective").is_err());
    assert!(corpus.synthesis("R001", &[]).is_err());
    assert!(
        corpus
            .synthesis("R001", &["unit".into(), "unit".into()])
            .is_err()
    );
    assert!(corpus.synthesis("R001", &["../escape".into()]).is_err());
}

#[test]
fn single_investigation_owns_exactly_one_reserved_item() {
    let (_temp, corpus) = corpus();
    let plan = corpus
        .investigation("R001", "Reproduce the baseline.")
        .unwrap();
    assert_eq!(plan.mode, WorkMode::Investigation);
    assert_eq!(plan.context_files, vec!["dir:research/R001-study"]);
    assert!(
        plan.instructions
            .contains("Question, Method, Result, Limitations and Next")
    );
    assert!(plan.instructions.contains("Do not edit other records"));
    assert!(corpus.investigation("R001", "  ").is_err());
    assert!(corpus.investigation("R002", "objective").is_err());
}

#[test]
fn forged_write_scope_is_rejected_before_backend_invocation() {
    use orbit_research_core::backend::{BackendConfig, OrbitBackend};
    let (temp, corpus) = corpus();
    let backend = OrbitBackend::new(
        BackendConfig {
            executable: temp.path().join("must-not-run"),
            workspace: "ws_fixture".into(),
            checkout: temp.path().to_owned(),
            owner_machine_id: "fixture".into(),
        },
        vec![],
    )
    .unwrap();
    let mut plan = corpus.investigation("R001", "objective").unwrap();
    plan.context_files = vec!["dir:research".into()];
    let error = corpus
        .link_work(&backend, "request", "work", "luna", &plan)
        .unwrap_err();
    assert!(error.to_string().contains("write scope"), "{error}");
    assert!(!temp.path().join(".git/orbit-research-operations").exists());
}
