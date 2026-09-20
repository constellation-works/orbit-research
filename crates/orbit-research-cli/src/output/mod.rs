use serde_json::Value;
use std::io::{self, Write};

/// Structured CLI failure rendered only on stderr.
pub(crate) struct Invalid {
    pub(crate) message: String,
    pub(crate) problems: Option<Vec<Value>>,
}

impl From<String> for Invalid {
    fn from(message: String) -> Self {
        Self {
            message,
            problems: None,
        }
    }
}

impl From<orbit_research_core::legacy_index::IndexError> for Invalid {
    fn from(error: orbit_research_core::legacy_index::IndexError) -> Self {
        Self {
            message: error.to_string(),
            problems: error.problems().map(|items| {
                items
                    .iter()
                    .map(orbit_research_core::legacy_index::Problem::to_value)
                    .collect()
            }),
        }
    }
}

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

pub(crate) fn invalid(error: Invalid, code: u8) -> std::process::ExitCode {
    let mut payload = serde_json::json!({"error":{"code":"invalid-input","message":error.message}});
    if let Some(problems) = error.problems {
        payload["error"]["problems"] = serde_json::json!(problems);
    }
    emit(io::stderr().lock(), &payload);
    std::process::ExitCode::from(code)
}

pub(crate) fn emit(mut stream: impl io::Write, value: &Value) {
    let _ = serde_json::to_writer(&mut stream, value);
    let _ = stream.write_all(b"\n");
}
