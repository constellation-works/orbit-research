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

/// Commit `research/<directory>/artifacts/<unit>/findings.md`, as a merged contribution does.
fn commit_findings(root: &Path, directory: &str, unit: &str) {
    let path = root.join(format!("research/{directory}/artifacts/{unit}"));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("findings.md"), "Findings.\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "contribution"]);
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
    let (temp, corpus) = corpus();
    commit_findings(temp.path(), "R001-study", "control-a");
    commit_findings(temp.path(), "R001-study", "control-b");
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

#[test]
fn synthesis_refuses_a_contribution_that_was_never_committed() {
    let (temp, corpus) = corpus();
    commit_findings(temp.path(), "R001-study", "control-a");
    let error = synthesis(&corpus, "R001", &["control-a".into(), "control-b".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("no contribution named control-b"), "{error}");
    assert!(error.contains("artifacts/control-b/findings.md"), "{error}");
}

#[test]
fn investigation_titles_carry_a_short_form_of_the_objective() {
    let (_temp, corpus) = corpus();
    let plan = investigation(&corpus, "R001", "Reproduce the baseline.\nSecond line.").unwrap();
    assert_eq!(plan.title, "Investigate R001: Reproduce the baseline.");
    let long = "Measure how the cache warm-up step behaves when the workspace was restored from an older snapshot";
    let plan = investigation(&corpus, "R001", long).unwrap();
    assert_eq!(
        plan.title,
        "Investigate R001: Measure how the cache warm-up step behaves when the…"
    );
    let plan = contribution(&corpus, "R001", "control-a", "Measure it").unwrap();
    assert_eq!(plan.title, "Contribute control-a to R001: Measure it");
}

#[test]
fn investigation_of_a_done_item_is_refused() {
    let (temp, corpus) = corpus();
    let path = temp.path().join("research/R001-study/README.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("status: planned", "status: done")).unwrap();
    git(temp.path(), &["commit", "-q", "-am", "done"]);
    let error = investigation(&corpus, "R001", "Again")
        .unwrap_err()
        .to_string();
    assert!(error.contains("R001 is already done"), "{error}");
    // Other shapes keep working: a done item can still gain a contribution plan.
    assert!(contribution(&corpus, "R001", "control-a", "More").is_ok());
}

#[test]
fn link_accepts_exactly_the_scopes_plan_derives() {
    let (temp, corpus) = corpus();
    commit_findings(temp.path(), "R001-study", "control-a");
    let plans = [
        investigation(&corpus, "R001", "Measure the control").unwrap(),
        contribution(&corpus, "R001", "control-a", "Measure it").unwrap(),
        contribution(&corpus, "R001", "control-b", "Measure it").unwrap(),
        synthesis(&corpus, "R001", &["control-a".into()]).unwrap(),
    ];
    for (index, plan) in plans.iter().enumerate() {
        let linked = corpus
            .link_intent(&format!("scope-{index}"), "R001", Some(&plan.context_files))
            .unwrap();
        assert_eq!(linked.context_files, plan.context_files);
        assert_eq!(linked.link.context_files, plan.context_files);
    }
    let default = corpus.link_intent("default", "R001", None).unwrap();
    assert_eq!(default.context_files, plans[0].context_files);

    for scope in [
        vec!["dir:research/R001-study/code/control-a".to_owned()],
        vec![
            "dir:research/R001-study/code/control-a".to_owned(),
            "dir:research/R001-study/artifacts/control-b".to_owned(),
        ],
        vec![
            "dir:research/R001-study/code/Control".to_owned(),
            "dir:research/R001-study/artifacts/Control".to_owned(),
        ],
        vec!["dir:research".to_owned()],
    ] {
        let error = corpus
            .link_intent("refused", "R001", Some(&scope))
            .unwrap_err();
        assert!(matches!(error, orbit_research_core::Error::InvalidInput(_)));
    }
    let error = corpus
        .link_intent("scope-0", "R001", Some(&plans[1].context_files))
        .unwrap_err();
    assert!(matches!(error, orbit_research_core::Error::Conflict(_)));
}
