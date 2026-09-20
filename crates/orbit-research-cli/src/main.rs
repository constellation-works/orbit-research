#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use serde_json::{Value, json};

use crate::command::application::compose;
use crate::output::{Invalid, OutputSink};
use crate::parse::{Cli, Command, ResearchOperation, WorkspaceOperation};

mod command;
mod mcp;
mod output;
mod parse;
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
    match Cli::try_parse_from(&args) {
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

fn fail(error: Invalid, code: u8, sink: &OutputSink) -> ExitCode {
    let _ = output::render_error(&mut io::stderr().lock(), &error, sink);
    ExitCode::from(code)
}

fn run(cli: Cli) -> ExitCode {
    let sink = OutputSink::from_process(cli.format);
    if let Command::Mcp { corpus } = &cli.command {
        let application = match compose(corpus, cli.backend_config.as_deref()) {
            Ok(application) => application,
            Err(error) => return fail(error, 1, &sink),
        };
        return match mcp::serve_mcp_application(
            &application,
            io::stdin().lock(),
            io::stdout().lock(),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
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
    let backend_config = cli.backend_config;
    match cli.command {
        Command::Mcp { .. } => Err("MCP must run as a stdio session".to_owned().into()),
        Command::Serve { corpus, port } => {
            let app = compose(&corpus, backend_config.as_deref())?;
            orbit_research_web::serve_application(app, port).map_err(|error| error.to_string())?;
            Ok((json!({"stopped":true}), 0))
        }
        Command::Workspace {
            operation: WorkspaceOperation::Init { path },
        } => Ok((command::workspace::initialize(&path)?, 0)),
        Command::Research {
            operation: ResearchOperation::Show { corpus, id },
        } => {
            let application = compose(&corpus, backend_config.as_deref())?;
            Ok((command::research::show(&application, &id)?, 0))
        }
        Command::Research { operation } => {
            let (corpus, operation, input) = command::research::prepare(operation)?;
            Ok((
                compose(&corpus, backend_config.as_deref())?
                    .execute(operation, input)
                    .map_err(|error| error.to_string())?,
                0,
            ))
        }
        Command::Resource { .. } => Ok(command::resource::render()),
    }
}
