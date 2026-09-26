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
