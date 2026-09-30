#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, Write};
use std::process::ExitCode;

use clap::CommandFactory;
use serde_json::Value;

use crate::command::application::compose;
use crate::output::{Invalid, OutputSink};
use crate::parse::{Cli, Command, WorkspaceOperation};

mod command;
mod mcp;
mod output;
mod parse;
mod plugin;
#[cfg(test)]
mod tests;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 1 {
        let mut out = io::stdout().lock();
        return finish_output(
            Cli::command()
                .write_long_help(&mut out)
                .and_then(|()| writeln!(out)),
        );
    }
    match Cli::try_parse_checked_from(&args) {
        Ok(cli) => run(cli),
        Err(error) if error.exit_code() == 0 => {
            finish_output(write!(io::stdout().lock(), "{error}"))
        }
        Err(error) => {
            let sink = OutputSink::from_process(output::error_format(&args));
            fail(error.to_string().into(), 2, &sink)
        }
    }
}

fn finish_output(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}

// Transport errors can originate from stdin or stdout. Observe the output sink
// so only a closed stdout pipe receives the successful pipeline exit code.
struct ProtocolWriter<W> {
    writer: W,
    broken_pipe: bool,
}

impl<W> ProtocolWriter<W> {
    fn observe<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        self.broken_pipe |= result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe);
        result
    }
}

impl<W: Write> Write for ProtocolWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let result = self.writer.write(buffer);
        self.observe(result)
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.writer.flush();
        self.observe(result)
    }
}

fn is_stdout_broken_pipe(error: &orbit_research_core::Error, observed: bool) -> bool {
    observed
        && match error {
            orbit_research_core::Error::Io(error) => error.kind() == io::ErrorKind::BrokenPipe,
            orbit_research_core::Error::Json(error) => {
                error.io_error_kind() == Some(io::ErrorKind::BrokenPipe)
            }
            _ => false,
        }
}

fn run_protocol(
    operation: impl FnOnce(&mut dyn Write) -> orbit_research_core::Result<()>,
) -> ExitCode {
    let mut output = ProtocolWriter {
        writer: io::stdout().lock(),
        broken_pipe: false,
    };
    match operation(&mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if is_stdout_broken_pipe(&error, output.broken_pipe) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}

fn fail(error: Invalid, code: u8, sink: &OutputSink) -> ExitCode {
    let _ = output::render_error(&mut io::stderr().lock(), &error, sink);
    ExitCode::from(code)
}

fn run(cli: Cli) -> ExitCode {
    let sink = OutputSink::from_process(if cli.json {
        output::OutputMode::Json
    } else {
        cli.format
    });
    if let Command::Mcp { corpus } = &cli.command {
        let application = match compose(corpus) {
            Ok(application) => application,
            Err(error) => return fail(error, 1, &sink),
        };
        return run_protocol(|output| {
            mcp::serve_mcp_application(&application, io::stdin().lock(), output)
        });
    }
    if let Command::OrbitTool = &cli.command {
        return run_protocol(|output| plugin::serve_plugin_tool_call(io::stdin().lock(), output));
    }
    match execute(cli) {
        Ok((value, code)) => {
            let rendered = output::render(
                &mut io::stdout().lock(),
                &mut io::stderr().lock(),
                &value,
                &sink,
            );
            if rendered.is_ok() {
                ExitCode::from(code)
            } else {
                finish_output(rendered)
            }
        }
        Err(error) => fail(error, 1, &sink),
    }
}

fn execute(cli: Cli) -> Result<(Value, u8), Invalid> {
    match cli.command {
        Command::Mcp { .. } => Err("MCP must run as a stdio session".to_owned().into()),
        Command::OrbitTool => Err("orbit-tool must run as a stdio session".to_owned().into()),
        Command::Workspace {
            operation: WorkspaceOperation::Init { path },
        } => Ok((command::workspace::initialize(&path)?, 0)),
        Command::Research { operation } => {
            let (corpus, operation, input) = command::research::prepare(*operation)?;
            Ok((
                compose(&corpus)?
                    .execute(operation, input)
                    .map_err(|error| error.to_string())?,
                0,
            ))
        }
        Command::Resource { .. } => Ok(command::resource::render()),
    }
}
