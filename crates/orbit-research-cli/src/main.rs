#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::Parser;
use serde_json::{Value, json};

use crate::command::application::compose;
use crate::output::{Invalid, invalid};
use crate::parse::{Cli, Command, WorkspaceOperation};

mod command;
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
        return match orbit_research_mcp::serve_mcp_application(
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
            let (corpus, name, input) = command::research::prepare(operation)?;
            Ok((
                compose(&corpus, backend_config.as_deref())?
                    .call(name, input)
                    .map_err(|error| error.to_string())?,
                0,
            ))
        }
        Command::Resource { .. } => Ok(command::legacy::resource()),
        Command::TaskContext {
            orbit_root,
            host,
            workspace,
            task,
            run,
            orbit_executable,
        } => command::task_context::execute(
            &orbit_root,
            &host,
            &workspace,
            &task,
            &run,
            &orbit_executable,
        ),
        Command::Validate { input, targets } => command::legacy::validate_file(&input, &targets),
        Command::Reconcile {
            input,
            targets,
            output,
        } => command::legacy::reconcile_file(&input, &targets, &output),
        Command::Index { config, database } => command::legacy::index(&config, &database),
        Command::IndexTrace { database, key } => command::legacy::index_trace(&database, &key),
        Command::BrowseExport {
            config,
            database,
            output,
        } => command::legacy::browse_export(&config, &database, &output),
        Command::Import {
            adapter,
            source_root,
            repository,
            expect_revision,
            select,
            output,
            ..
        } => command::legacy::import_file(
            &adapter,
            &source_root,
            &repository,
            expect_revision.as_deref(),
            &select,
            output.as_deref(),
        ),
        Command::Program(args) => command::legacy::author("program", &args),
        Command::Claim(args) => command::legacy::author("claim", &args),
        Command::Artifact(args) => command::legacy::author("artifact", &args),
        Command::Preregister(args) => command::legacy::author("preregister", &args),
        Command::BeginRun(args) => command::legacy::author("begin-run", &args),
        Command::RecordRun(args) => command::legacy::author("record-run", &args),
        Command::Assess(args) => command::legacy::author("assess", &args),
        Command::Retire(args) => command::legacy::author("retire", &args),
        Command::Heads { owner, id } => command::legacy::owner_heads(owner, &id),
        Command::Ref {
            owner,
            id,
            revision,
            source_revision,
        } => command::legacy::owner_ref(owner, &id, &revision, &source_revision),
        Command::Trace {
            owner,
            id,
            revision,
        } => command::legacy::owner_trace(owner, &id, &revision),
        Command::Export {
            owner,
            source_revision,
            output,
        } => command::legacy::owner_export(owner, &source_revision, &output),
    }
}
