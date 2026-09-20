use super::super::{
    sink::{Mode, OutputMode, OutputSink},
    table::render_records,
};

fn sink(mode: Mode, width: usize) -> OutputSink {
    match mode {
        Mode::Table => OutputSink::resolve(OutputMode::Table, true, width, false),
        Mode::Plain => OutputSink::resolve(OutputMode::Auto, false, width, false),
        Mode::Json => OutputSink::resolve(OutputMode::Json, false, width, false),
        Mode::Ndjson => OutputSink::resolve(OutputMode::Ndjson, false, width, false),
    }
}

fn records() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": "R001", "kind": "research", "path": "research/deep/first-record.md",
            "metadata": {"status": "open", "title": "A useful question", "tags": ["rust", "orbit"]}
        }),
        serde_json::json!({
            "id": "R002", "kind": "research", "path": "研究/second-record.md",
            "metadata": {"status": "closed", "title": "界面 behavior", "tags": ["cli"]}
        }),
    ]
}

#[test]
fn plain_is_headerless_untruncated_and_row_safe() {
    let records = vec![serde_json::json!({
        "id": "R\t1", "kind": "research", "path": "a\npath",
        "metadata": {"status": "open", "title": "line\rtitle", "tags": ["one\ttwo"]}
    })];
    let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
    render_records(&mut out, &mut diagnostics, &records, &sink(Mode::Plain, 4))
        .expect("plain records render");
    assert_eq!(
        String::from_utf8(out).expect("utf8 output"),
        "R\\t1\tresearch\topen\tline\\rtitle\tone\\ttwo\ta\\npath\n"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn table_aligns_by_unicode_display_width_and_preserves_rows() {
    let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
    render_records(
        &mut out,
        &mut diagnostics,
        &records(),
        &sink(Mode::Table, 0),
    )
    .expect("table records render");
    let output = String::from_utf8(out).expect("utf8 output");
    assert_eq!(output.lines().count(), 3);
    assert_eq!(
        output,
        "ID    KIND      STATUS  TITLE              TAGS        PATH\nR001  research  open    A useful question  rust,orbit  research/deep/first-record.md\nR002  research  closed  界面 behavior      cli         研究/second-record.md\n"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn narrow_tables_shrink_then_drop_from_the_right() {
    let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
    render_records(
        &mut out,
        &mut diagnostics,
        &records(),
        &sink(Mode::Table, 35),
    )
    .expect("narrow table renders");
    let output = String::from_utf8(out).expect("utf8 output");
    assert_eq!(output.lines().count(), 3);
    assert!(
        output
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 35)
    );
    assert!(output.contains("R001"));
    assert!(output.contains("R002"));
    assert_eq!(
        String::from_utf8(diagnostics).expect("utf8 diagnostics"),
        "Dropped PATH column to fit terminal width.\nDropped TAGS column to fit terminal width.\n"
    );
}

#[test]
fn empty_human_results_only_write_one_diagnostic() {
    for mode in [Mode::Table, Mode::Plain] {
        let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
        render_records(&mut out, &mut diagnostics, &[], &sink(mode, 80))
            .expect("empty results render");
        assert!(out.is_empty());
        assert_eq!(diagnostics, b"No research records found.\n");
    }
}

#[test]
fn dim_color_is_limited_to_headers() {
    let mut configured = sink(Mode::Table, 0);
    configured.color = true;
    let (mut out, mut diagnostics) = (Vec::new(), Vec::new());
    render_records(&mut out, &mut diagnostics, &records()[..1], &configured)
        .expect("colored table renders");
    let output = String::from_utf8(out).expect("utf8 output");
    assert!(
        output
            .lines()
            .next()
            .is_some_and(|line| line.contains("\x1b[2mID\x1b[0m"))
    );
    assert!(!output.lines().nth(1).unwrap_or_default().contains('\x1b'));
}
