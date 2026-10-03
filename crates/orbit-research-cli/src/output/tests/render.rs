use super::super::{OutputMode, OutputSink, render};

fn render_with_terminal(
    out: &mut impl std::io::Write,
    value: &serde_json::Value,
    mode: OutputMode,
    tty: bool,
) -> std::io::Result<()> {
    render(
        out,
        &mut Vec::new(),
        value,
        &OutputSink::resolve(mode, tty, 0, false),
    )
}
use serde_json::Value;

#[test]
fn json_and_ndjson_are_machine_stable() {
    let value = serde_json::json!([{"id":"R001"},{"id":"R002"}]);
    let mut json = Vec::new();
    render_with_terminal(&mut json, &value, OutputMode::Json, false)
        .expect("fixture output should encode and decode");
    assert_eq!(
        serde_json::from_slice::<Value>(&json).expect("fixture output should encode and decode"),
        value
    );
    let mut ndjson = Vec::new();
    render_with_terminal(&mut ndjson, &value, OutputMode::Ndjson, false)
        .expect("fixture output should encode and decode");
    assert_eq!(
        String::from_utf8(ndjson).expect("fixture output should encode and decode"),
        include_str!("../../snapshots/research-list.ndjson")
    );
}

#[test]
fn table_is_plain_and_untruncated() {
    let mut output = Vec::new();
    render_with_terminal(
        &mut output,
        &serde_json::json!({"id":"R001","title":"A title"}),
        OutputMode::Table,
        false,
    )
    .expect("fixture output should encode and decode");
    let output = String::from_utf8(output).expect("fixture output should encode and decode");
    assert!(output.contains("R001"));
    assert!(!output.contains('\u{1b}'));
}

#[test]
fn snapshot_records_render_as_compact_table_in_human_modes() {
    let value = serde_json::json!({
        "revision": "abc123",
        "records": [{
            "id": "R001",
            "kind": "R",
            "path": "research/R001-example.md",
            "metadata": {"status": "open", "title": "A useful question", "tags": ["rust", "orbit"]},
            "body": "long body stays out of the table",
            "content_sha256": "hash",
            "git_blob": "blob"
        }],
        "tags": ["rust", "orbit"]
    });
    for mode in [OutputMode::Table, OutputMode::Auto] {
        let mut output = Vec::new();
        render_with_terminal(&mut output, &value, mode, true)
            .expect("renderer fixture should succeed");
        let output = String::from_utf8(output).expect("renderer fixture should succeed");
        assert!(output.contains("ID") && output.contains("TITLE"));
        assert!(output.contains("R001") && output.contains("A useful question"));
        assert!(!output.contains("long body"));
    }
}

#[test]
fn auto_pipe_output_is_plain_and_untruncated() {
    let value = serde_json::json!({
        "records": [{
            "id": "R001",
            "kind": "R",
            "path": "research/R001-example.md",
            "metadata": {"status": "open", "title": "A useful question", "tags": []}
        }]
    });
    let mut output = Vec::new();
    render_with_terminal(&mut output, &value, OutputMode::Auto, false)
        .expect("renderer fixture should succeed");
    let output = String::from_utf8(output).expect("renderer fixture should succeed");
    assert_eq!(
        output,
        "R001\tR\topen\tA useful question\t\tresearch/R001-example.md\n"
    );
    assert!(!output.contains('\u{1b}'));
}

#[test]
fn snapshot_ndjson_emits_one_record_per_line() {
    let value = serde_json::json!({
        "revision": "abc123",
        "records": [{"id": "R001", "kind": "R"}, {"id": "R002", "kind": "R"}],
        "tags": []
    });
    let mut output = Vec::new();
    render_with_terminal(&mut output, &value, OutputMode::Ndjson, false)
        .expect("renderer fixture should succeed");
    assert_eq!(
        String::from_utf8(output)
            .expect("renderer fixture should succeed")
            .lines()
            .count(),
        2
    );
}

#[test]
fn validation_summary_is_concise_for_humans_and_structured_for_machines() {
    let value = serde_json::json!({
        "valid": true,
        "base_revision": "abc123",
        "record_count": 1,
        "tag_count": 2
    });
    let mut human = Vec::new();
    render_with_terminal(&mut human, &value, OutputMode::Auto, false)
        .expect("summary rendering should succeed");
    assert_eq!(
        String::from_utf8(human).expect("summary output is UTF-8"),
        "Corpus validation passed at base revision abc123: 1 record, 2 tags.\n"
    );

    let mut json = Vec::new();
    render_with_terminal(&mut json, &value, OutputMode::Json, false)
        .expect("JSON output should succeed");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json).expect("summary remains JSON"),
        value
    );

    let mut ndjson = Vec::new();
    render_with_terminal(&mut ndjson, &value, OutputMode::Ndjson, false)
        .expect("NDJSON output should succeed");
    let lines = String::from_utf8(ndjson).expect("summary output is UTF-8");
    assert_eq!(lines.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines.trim())
            .expect("summary remains one JSON line"),
        value
    );
}

fn human(value: &serde_json::Value) -> String {
    let mut output = Vec::new();
    render_with_terminal(&mut output, value, OutputMode::Auto, false)
        .expect("human rendering succeeds");
    String::from_utf8(output).expect("UTF-8 output")
}

#[test]
fn show_prints_metadata_first_with_numbered_assessments_and_the_body_last() {
    let value = serde_json::json!({
        "id": "H001", "kind": "H", "path": "hypotheses/H001-h.md",
        "git_blob": "blob", "content_sha256": "sha",
        "body": "\n# H001 — Claim\n\n## The claim\n\ntext\n",
        "metadata": {
            "id": "H001", "title": "Claim", "status": "open", "tags": ["a", "b"],
            "derived_from": ["Q001"], "created": "2026-09-01", "updated": "2026-09-02",
            "revision": 2,
            "assessments": [
                {"date": "2026-09-01", "research": "R001", "revision": 1, "verdict": "supports", "strength": "strong"},
                {"date": "2026-09-05", "research": "R002", "revision": 2, "verdict": "refutes", "strength": "anecdote", "note": "failed controls"}
            ]
        }
    });
    assert_eq!(
        human(&value),
        "id: H001\nkind: H\ntitle: Claim\nstatus: open\npath: hypotheses/H001-h.md\n\
         tags: a, b\nderived_from: Q001\nrevision: 2\ncreated: 2026-09-01\nupdated: 2026-09-02\n\
         assessments:\n  [1] 2026-09-01 R001 revision 1 supports (strong)\n  \
         [2] 2026-09-05 R002 revision 2 refutes (anecdote)\n      note: failed controls\n\
         git_blob: blob\ncontent_sha256: sha\n\nbody:\n# H001 — Claim\n\n## The claim\n\ntext\n"
    );
}

#[test]
fn plan_drafts_render_text_readably_with_numbered_lists() {
    let value = serde_json::json!({
        "title": "Investigate R001: Test",
        "description": "First line.\n\nSecond paragraph.",
        "acceptance_criteria": ["Documents Question, Method", "Is bound to the run"],
        "context_files": ["dir:research/R001-x"]
    });
    assert_eq!(
        human(&value),
        "acceptance_criteria[1]: Documents Question, Method\n\
         acceptance_criteria[2]: Is bound to the run\n\
         context_files: dir:research/R001-x\n\
         description:\n  First line.\n\n  Second paragraph.\n\
         title: Investigate R001: Test\n"
    );
}

#[test]
fn lists_of_records_are_separated_and_empty_ones_say_so_on_stderr() {
    let value = serde_json::json!([
        {"request_key": "a", "research_id": "R001", "task_id": null},
        {"request_key": "b", "research_id": "R002", "task_id": "ORB-2"}
    ]);
    assert_eq!(
        human(&value),
        "request_key: a\nresearch_id: R001\ntask_id: -\n\nrequest_key: b\nresearch_id: R002\ntask_id: ORB-2\n"
    );
    let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
    render(
        &mut out,
        &mut diagnostics,
        &serde_json::json!([]),
        &OutputSink::resolve(OutputMode::Auto, false, 0, false),
    )
    .expect("render");
    assert!(out.is_empty());
    assert_eq!(diagnostics, b"No work links found.\n");
}

#[test]
fn the_packaged_resource_ends_with_exactly_one_newline() {
    let value = serde_json::json!({"version": 1, "skill": "Body\nlast line\n"});
    assert_eq!(human(&value), "Body\nlast line\n");
}

#[test]
fn pipe_output_escapes_tabs_newlines_and_backslashes_in_fields() {
    let value = serde_json::json!({"records": [{
        "id": "Q001", "kind": "Q", "path": "questions/Q001-x.md",
        "metadata": {"status": "open", "title": "a\tb\nc\\t", "tags": ["x\ty"]}
    }]});
    let output = human(&value);
    assert_eq!(
        output,
        "Q001\tQ\topen\ta\\tb\\nc\\\\t\tx\\ty\tquestions/Q001-x.md\n"
    );
    assert_eq!(output.matches('\t').count(), 5);
    assert_eq!(output.matches('\n').count(), 1);
}
