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
