use std::io::{self, Write};

use orbit_research_core::Error;

use super::super::{ProtocolWriter, is_stdout_broken_pipe};

#[test]
fn only_observed_stdout_broken_pipes_are_quiet() {
    let broken_pipe = Error::Io(io::Error::from(io::ErrorKind::BrokenPipe));
    assert!(!is_stdout_broken_pipe(&broken_pipe, false));
    assert!(is_stdout_broken_pipe(&broken_pipe, true));
    let other_io = Error::Io(io::Error::from(io::ErrorKind::PermissionDenied));
    assert!(!is_stdout_broken_pipe(&other_io, true));
    assert!(!is_stdout_broken_pipe(
        &Error::Invalid("Broken pipe".into()),
        true
    ));
}

struct ClosedPipe;

impl Write for ClosedPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn json_serialization_retains_the_observed_stdout_error() {
    let mut output = ProtocolWriter {
        writer: ClosedPipe,
        broken_pipe: false,
    };
    let error = serde_json::to_writer(&mut output, &serde_json::json!({"result":{}}))
        .expect_err("closed output pipe")
        .into();
    assert!(output.broken_pipe);
    assert!(is_stdout_broken_pipe(&error, output.broken_pipe));
}
