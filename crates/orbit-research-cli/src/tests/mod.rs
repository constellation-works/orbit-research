#[cfg(test)]
mod output {
    use crate::output::*;
    use serde_json::Value;

    #[test]
    fn json_and_ndjson_are_machine_stable() {
        let value = serde_json::json!([{"id":"R001"},{"id":"R002"}]);
        let mut json = Vec::new();
        render_with_terminal(&mut json, &value, OutputMode::Json, false)
            .expect("fixture output should encode and decode");
        assert_eq!(
            serde_json::from_slice::<Value>(&json)
                .expect("fixture output should encode and decode"),
            value
        );
        let mut ndjson = Vec::new();
        render_with_terminal(&mut ndjson, &value, OutputMode::Ndjson, false)
            .expect("fixture output should encode and decode");
        assert_eq!(
            String::from_utf8(ndjson).expect("fixture output should encode and decode"),
            include_str!("../snapshots/research-list.ndjson")
        );
    }

    #[test]
    fn table_is_plain_and_untruncated() {
        let mut output = Vec::new();
        render_with_terminal(
            &mut output,
            &serde_json::json!({"id":"R001","title":"A title"}),
            OutputMode::Table,
            false,
        )
        .expect("fixture output should encode and decode");
        let output = String::from_utf8(output).expect("fixture output should encode and decode");
        assert!(output.contains("R001"));
        assert!(!output.contains('\u{1b}'));
    }
}

#[cfg(test)]
mod parse {
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
}
