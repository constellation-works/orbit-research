use orbit_research_core::{Research as Corpus, work::PlanShape};
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

fn contribution(
    corpus: &Corpus,
    research_id: &str,
    unit: &str,
    objective: &str,
) -> Result<orbit_research_core::work::TaskDraft, orbit_research_core::Error> {
    corpus.plan(
        research_id,
        PlanShape::Contribution {
            unit: unit.into(),
            objective: objective.into(),
        },
    )
}

fn synthesis(
    corpus: &Corpus,
    research_id: &str,
    units: &[String],
) -> Result<orbit_research_core::work::TaskDraft, orbit_research_core::Error> {
    corpus.plan(
        research_id,
        PlanShape::Synthesis {
            units: units.to_vec(),
        },
    )
}

fn investigation(
    corpus: &Corpus,
    research_id: &str,
    objective: &str,
) -> Result<orbit_research_core::work::TaskDraft, orbit_research_core::Error> {
    corpus.plan(
        research_id,
        PlanShape::Investigation {
            objective: objective.into(),
        },
    )
}

#[test]
fn contribution_owns_disjoint_unit_paths() {
    let (_temp, corpus) = corpus();
    let plan = contribution(&corpus, "R001", "control-a", "Measure the control.").unwrap();
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
        plan.description
            .contains("do not edit it or the shared data/manifest.json")
    );
    assert!(
        plan.acceptance_criteria
            .iter()
            .any(|criterion| criterion.contains("findings.md"))
    );
}

#[test]
fn synthesis_scopes_shared_summary_and_manifest() {
    let (_temp, corpus) = corpus();
    let plan = synthesis(&corpus, "R001", &["control-a".into(), "control-b".into()]).unwrap();
    assert_eq!(
        plan.context_files,
        vec![
            "file:research/R001-study/README.md",
            "file:research/R001-study/data/manifest.json",
        ]
    );
    assert!(
        plan.description
            .contains("research/R001-study/artifacts/control-a/findings.md")
    );
    assert!(
        plan.description
            .contains("research/R001-study/artifacts/control-b/findings.md")
    );
}

#[test]
fn work_rejects_traversal_missing_research_and_incomplete_synthesis() {
    let (_temp, corpus) = corpus();
    assert!(contribution(&corpus, "R001", "../escape", "objective").is_err());
    assert!(contribution(&corpus, "Q001", "unit", "objective").is_err());
    assert!(synthesis(&corpus, "R001", &[]).is_err());
    assert!(synthesis(&corpus, "R001", &["unit".into(), "unit".into()]).is_err());
    assert!(synthesis(&corpus, "R001", &["../escape".into()]).is_err());
}

#[test]
fn single_investigation_owns_exactly_one_reserved_item() {
    let (_temp, corpus) = corpus();
    let plan = investigation(&corpus, "R001", "Reproduce the baseline.").unwrap();
    assert_eq!(plan.context_files, vec!["dir:research/R001-study"]);
    assert!(
        plan.acceptance_criteria
            .iter()
            .any(|criterion| criterion.contains("Question, Method, Result, Limitations and Next"))
    );
    assert!(plan.description.contains("Do not edit other records"));
    assert!(investigation(&corpus, "R001", "  ").is_err());
    assert!(investigation(&corpus, "R002", "objective").is_err());
}

#[test]
fn planning_uses_committed_content_while_browsing_ignores_uncommitted_edits() {
    let (temp, corpus) = corpus();
    let before = investigation(&corpus, "R001", "Measure the control").unwrap();
    let path = temp.path().join("research/R001-study/README.md");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("\nUncommitted local observation.\n");
    fs::write(&path, text).unwrap();
    let browse = corpus.snapshot().unwrap();
    assert!(
        browse.records[0]
            .body
            .contains("Uncommitted local observation")
    );
    let after = investigation(&corpus, "R001", "Measure the control").unwrap();
    assert_eq!(after.context_files, before.context_files);
}
