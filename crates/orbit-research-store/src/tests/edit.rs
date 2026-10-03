//! Edit policy runs on in-memory records: no Git, no filesystem.
use crate::{
    edit::{self, Assessment, Edit},
    record,
    validation::Contract,
};
use orbit_research_common::Record;
use serde_json::{Value, json};

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn contract() -> Contract {
    Contract::compile(serde_json::from_slice(SCHEMA).expect("schema JSON")).expect("contract")
}

fn record(kind: &str, path: &str, text: &str) -> Record {
    let (metadata, body) = record::parse(text).expect("fixture record");
    Record {
        id: metadata["id"].as_str().expect("id").into(),
        kind: kind.into(),
        path: path.into(),
        metadata,
        body,
        content_sha256: String::new(),
        git_blob: String::new(),
    }
}

const H001: &str = "---\nid: H001\ntitle: Claim\nstatus: supported\ntags: [x]\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\nrevision: 1\nassessments:\n  - date: 2026-01-02\n    research: R001\n    revision: 1\n    verdict: supports\n    strength: suggestive\n    note: first\n---\n\n## The claim\n\nOriginal statement.\n";

const R001: &str = "---\nid: R001\ntitle: Study\nstatus: done\ntags: []\nderived_from: []\ncreated: 2026-01-01\nupdated: 2026-01-01\ntests: [H001]\n---\n\n## Result\n\nControls failed.\n";

fn corpus() -> Vec<Record> {
    vec![
        record("H", "hypotheses/H001-claim.md", H001),
        record("R", "research/R001-study/README.md", R001),
    ]
}

fn parsed(text: &str) -> Value {
    record::parse(text).expect("rendered record").0
}

fn assessment(revision: u64, verdict: &str) -> Assessment {
    Assessment {
        research: "R001".into(),
        revision,
        verdict: verdict.into(),
        strength: "suggestive".into(),
        note: Some("controls failed".into()),
    }
}

#[test]
fn hypothesis_statement_change_bumps_revision_and_keeps_assessments() {
    let records = corpus();
    let edit = Edit {
        body: Some("## The claim\n\nSharper statement.".into()),
        ..Edit::default()
    };
    let text = edit::revise(&contract(), &records[0], &edit, "2026-02-01").expect("revise");
    let metadata = parsed(&text);
    assert_eq!(metadata["revision"], 2);
    assert_eq!(metadata["status"], "open");
    assert_eq!(metadata["assessments"], records[0].metadata["assessments"]);
    assert_eq!(metadata["assessments"][0]["revision"], 1);
    assert_eq!(metadata["updated"], "2026-02-01");
    assert!(text.ends_with("\n## The claim\n\nSharper statement.\n"));
}

#[test]
fn hypothesis_tag_edit_keeps_its_revision_and_status() {
    let records = corpus();
    let edit = Edit {
        tags: Some(vec!["y".into()]),
        body: Some(records[0].body.trim().into()),
        ..Edit::default()
    };
    let metadata =
        parsed(&edit::revise(&contract(), &records[0], &edit, "2026-02-01").expect("revise"));
    assert_eq!(metadata["revision"], 1);
    assert_eq!(metadata["status"], "supported");
    assert_eq!(metadata["tags"], json!(["y"]));
}

#[test]
fn research_fields_are_refused_on_other_kinds() {
    let records = corpus();
    let edit = Edit {
        tests: Some(vec!["H001".into()]),
        ..Edit::default()
    };
    let error = edit::revise(&contract(), &records[0], &edit, "2026-02-01").unwrap_err();
    assert!(
        error.to_string().contains("only to research records"),
        "{error}"
    );
}

#[test]
fn assessment_appends_in_order_and_follows_verdict_status() {
    let records = corpus();
    let text = edit::assess(
        &contract(),
        &records,
        &records[0],
        &assessment(1, "inconclusive"),
        "2026-02-01",
    )
    .expect("assess");
    let metadata = parsed(&text);
    let entries = metadata["assessments"].as_array().expect("assessments");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], records[0].metadata["assessments"][0]);
    assert_eq!(
        entries[1],
        json!({"date": "2026-02-01", "research": "R001", "revision": 1, "verdict": "inconclusive", "strength": "suggestive", "note": "controls failed"})
    );
    assert_eq!(metadata["status"], "inconclusive");
    assert_eq!(metadata["revision"], 1);
}

#[test]
fn assessment_of_an_older_revision_leaves_current_status_alone() {
    let records = corpus();
    let edit = Edit {
        body: Some("Revised statement.".into()),
        ..Edit::default()
    };
    let revised = edit::revise(&contract(), &records[0], &edit, "2026-02-01").expect("revise");
    let revised = record("H", "hypotheses/H001-claim.md", &revised);
    let metadata = parsed(
        &edit::assess(
            &contract(),
            &records,
            &revised,
            &assessment(1, "refutes"),
            "2026-02-02",
        )
        .expect("assess"),
    );
    assert_eq!(metadata["status"], "open");
    assert_eq!(metadata["assessments"][1]["revision"], 1);
}

#[test]
fn dropped_hypothesis_stays_dropped() {
    let mut records = corpus();
    records[0].metadata["status"] = json!("dropped");
    let metadata = parsed(
        &edit::assess(
            &contract(),
            &records,
            &records[0],
            &assessment(1, "supports"),
            "2026-02-01",
        )
        .expect("assess"),
    );
    assert_eq!(metadata["status"], "dropped");
}

#[test]
fn assessment_refuses_missing_revisions_and_non_research_citations() {
    let records = corpus();
    for revision in [0, 2] {
        let error = edit::assess(
            &contract(),
            &records,
            &records[0],
            &assessment(revision, "supports"),
            "2026-02-01",
        )
        .unwrap_err();
        assert!(error.to_string().contains("has no revision"), "{error}");
    }
    let mut cites_hypothesis = assessment(1, "supports");
    cites_hypothesis.research = "H001".into();
    let error = edit::assess(
        &contract(),
        &records,
        &records[0],
        &cites_hypothesis,
        "2026-02-01",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("cite research records"),
        "{error}"
    );
    let error = edit::assess(
        &contract(),
        &records,
        &records[1],
        &assessment(1, "supports"),
        "2026-02-01",
    )
    .unwrap_err();
    assert!(error.to_string().contains("Only hypotheses"), "{error}");
}

#[test]
fn record_checks_catch_dangling_references_without_git() {
    let records = corpus();
    let edit = Edit {
        tests: Some(vec!["H999".into()]),
        ..Edit::default()
    };
    let text = edit::revise(&contract(), &records[1], &edit, "2026-02-01").expect("revise");
    let error = edit::check_records(&contract(), &records, &records[1].path, &text).unwrap_err();
    assert!(
        error.to_string().contains("references missing H999"),
        "{error}"
    );
}

#[test]
fn manifest_is_checked_against_the_owner_definition() {
    let text = edit::manifest_text(&contract(), &json!({"inputs": [{"name": "a.csv"}]}))
        .expect("valid manifest");
    assert!(text.ends_with("}\n"));
    let error = edit::manifest_text(&contract(), &json!({"inputs": [{}]})).unwrap_err();
    assert!(error.to_string().contains("data/manifest.json"), "{error}");
}

fn with_heading(record_text: &str, heading: &str) -> String {
    record_text.replace(
        "\n\n## The claim",
        &format!("\n\n{heading}\n\n## The claim"),
    )
}

#[test]
fn a_title_change_rewrites_the_scaffolded_body_heading() {
    let text = with_heading(H001, "# H001 — Claim");
    let record = record("H", "hypotheses/H001-claim.md", &text);
    let edit = Edit {
        title: Some("Sharper claim".into()),
        ..Edit::default()
    };
    let rendered = edit::revise(&contract(), &record, &edit, "2026-02-01").expect("revise");
    assert!(
        rendered.contains("\n\n# H001 — Sharper claim\n\n## The claim\n\nOriginal statement.\n"),
        "{rendered}"
    );
    assert!(!rendered.contains("# H001 — Claim"));
    assert_eq!(parsed(&rendered)["title"], "Sharper claim");
    // The body stays as-is when the title is unchanged.
    let same = Edit {
        tags: Some(vec!["y".into()]),
        ..Edit::default()
    };
    let rendered = edit::revise(&contract(), &record, &same, "2026-02-01").expect("revise");
    assert!(rendered.contains("# H001 — Claim"));
}

#[test]
fn a_heading_the_author_wrote_differently_is_left_alone() {
    let text = with_heading(H001, "# My own heading");
    let record = record("H", "hypotheses/H001-claim.md", &text);
    let edit = Edit {
        title: Some("Sharper claim".into()),
        ..Edit::default()
    };
    let rendered = edit::revise(&contract(), &record, &edit, "2026-02-01").expect("revise");
    assert!(rendered.contains("# My own heading"));
    assert!(!rendered.contains("Sharper claim\n\n## The claim"));
}

#[test]
fn an_edit_with_no_fields_is_empty() {
    assert!(Edit::default().is_empty());
    assert!(
        !Edit {
            status: Some("open".into()),
            ..Edit::default()
        }
        .is_empty()
    );
}
