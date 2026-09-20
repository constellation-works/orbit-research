use serde_json::Value;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum OutputMode {
    Auto,
    Table,
    Json,
    Ndjson,
}

pub fn render_with_terminal(
    mut writer: &mut impl Write,
    value: &Value,
    mode: OutputMode,
    interactive: bool,
) -> io::Result<()> {
    let mode = match mode {
        OutputMode::Auto if interactive => OutputMode::Table,
        OutputMode::Auto => OutputMode::Table,
        mode => mode,
    };
    match mode {
        OutputMode::Json => serde_json::to_writer(&mut writer, value)?,
        OutputMode::Ndjson => match value {
            Value::Array(values) => {
                for item in values {
                    serde_json::to_writer(&mut writer, item)?;
                    writer.write_all(b"\n")?;
                }
                return Ok(());
            }
            value => serde_json::to_writer(&mut writer, value)?,
        },
        OutputMode::Table | OutputMode::Auto => render_table(&mut writer, value)?,
    }
    writer.write_all(b"\n")
}

fn render_table(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if value.is_array() || value.is_object() {
                    writeln!(writer, "{key}: {}", value)?;
                } else {
                    writeln!(writer, "{key}\t{}", scalar(value))?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                writeln!(writer, "{}", value)?;
            }
        }
        value => writeln!(writer, "{}", scalar(value))?,
    }
    Ok(())
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "null".into(),
        value => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            String::from_utf8(ndjson)
                .expect("fixture output should encode and decode")
                .lines()
                .count(),
            2
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
