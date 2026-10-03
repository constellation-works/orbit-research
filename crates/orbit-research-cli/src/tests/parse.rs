use crate::parse::Cli;
use clap::Parser;

#[test]
fn removed_backend_surfaces_are_rejected_as_unknown() {
    assert!(
        Cli::try_parse_from(["orbit-research", "--backend-config", "/tmp/backend.json"]).is_err()
    );
    for subcommand in ["backend", "status", "link", "promote", "dispatch", "cancel"] {
        assert!(
            Cli::try_parse_from([
                "orbit-research",
                "research",
                subcommand,
                "--corpus",
                "/tmp/corpus",
            ])
            .is_err(),
            "research {subcommand} should be rejected as unknown"
        );
    }
    assert!(Cli::try_parse_from(["orbit-research", "serve", "--corpus", "/tmp/corpus"]).is_err());
}

fn help_of(args: &[&str]) -> String {
    let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
    Cli::try_parse_checked_from(&args)
        .expect_err("help exits through clap")
        .to_string()
}

#[test]
fn protocol_subcommand_help_hides_the_output_format_flags() {
    for args in [
        ["orbit-research", "mcp", "--help"].as_slice(),
        ["orbit-research", "orbit-tool", "--help"].as_slice(),
    ] {
        let help = help_of(args);
        assert!(
            !help.contains("--format") && !help.contains("--json"),
            "{help}"
        );
    }
    assert!(help_of(&["orbit-research", "mcp", "--help"]).contains("--corpus"));
    // Other commands still document them, and the hidden flags still parse.
    assert!(help_of(&["orbit-research", "research", "list", "--help"]).contains("--format"));
    assert!(Cli::try_parse_from(["orbit-research", "mcp", "--corpus", "/tmp/c", "--json"]).is_ok());
}
