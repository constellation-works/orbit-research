use super::super::sink::{Mode, OutputMode, OutputSink};

#[test]
fn pipe_never_inherits_terminal_width_or_color() {
    let sink = OutputSink::resolve(OutputMode::Auto, false, 40, true);
    assert_eq!(sink.mode, Mode::Plain);
    assert_eq!(sink.width, 0);
    assert!(!sink.color);
    assert!(!sink.suppress_uniform);
}

#[test]
fn explicit_modes_override_auto_without_styling_machine_output() {
    for mode in [OutputMode::Json, OutputMode::Ndjson] {
        let sink = OutputSink::resolve(mode, true, 80, true);
        assert!(sink.machine());
        assert!(!sink.color);
    }
    assert!(OutputSink::resolve(OutputMode::Auto, true, 80, false).suppress_uniform);
    assert!(!OutputSink::resolve(OutputMode::Table, true, 80, false).suppress_uniform);
}
