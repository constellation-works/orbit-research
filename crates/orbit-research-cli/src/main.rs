#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::Parser;
use serde_json::{Value, json};

use crate::command::application::compose;
use crate::output::{Invalid, invalid};
use crate::parse::{Cli, Command, WorkspaceOperation};

mod command;
mod mcp;
mod output;
mod parse;
#[cfg(test)]
mod tests;

fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => run(cli),
        Err(error) if error.exit_code() == 0 => {
            let _ = error.print();
            ExitCode::SUCCESS
        }
        Err(error) => invalid(error.to_string().into(), 2),
    }
}

fn run(cli: Cli) -> ExitCode {
    if let Command::Mcp { corpus } = &cli.command {
        let application = match compose(corpus, cli.backend_config.as_deref()) {
            Ok(application) => application,
            Err(error) => return invalid(error, 1),
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
    let error_code = if matches!(
        &cli.command,
        Command::Research { .. } | Command::Workspace { .. } | Command::Serve { .. }
    ) {
        1
    } else {
        2
    };
    let mode = cli.format;
    match execute(cli) {
        Ok((value, code)) => match output::render_with_terminal(
            &mut io::stdout().lock(),
            &value,
            mode,
            io::stdout().is_terminal(),
        ) {
            Ok(()) => ExitCode::from(code),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        },
        Err(error) => invalid(error, error_code),
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
