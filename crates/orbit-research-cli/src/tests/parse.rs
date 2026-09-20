use crate::parse::{Cli, Command, ResearchOperation};
use clap::Parser;

#[test]
fn backend_config_is_global_and_routes_explicit_work_commands() {
    let cli = Cli::try_parse_from([
        "orbit-research",
        "--backend-config",
        "/tmp/backend.json",
        "research",
        "dispatch",
        "--corpus",
        "/tmp/corpus",
        "--request-key",
        "req-1",
        "--base",
        "agent-main",
    ])
    .expect("global backend configuration should parse");
    assert_eq!(
        cli.backend_config
            .as_deref()
            .expect("configured backend path")
            .to_str(),
        Some("/tmp/backend.json")
    );
    assert!(matches!(
        cli.command,
        Command::Research {
            operation: ResearchOperation::Dispatch { .. }
        }
    ));
}
