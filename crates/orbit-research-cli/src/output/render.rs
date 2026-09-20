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

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum OutputMode {
    Auto,
    Table,
    Json,
    Ndjson,
}

#[derive(Clone, Copy)]
enum ResolvedOutputMode {
    Table,
    Plain,
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
        OutputMode::Auto if interactive => ResolvedOutputMode::Table,
        OutputMode::Auto => ResolvedOutputMode::Plain,
        OutputMode::Table => ResolvedOutputMode::Table,
        OutputMode::Json => ResolvedOutputMode::Json,
        OutputMode::Ndjson => ResolvedOutputMode::Ndjson,
    };
    match mode {
        ResolvedOutputMode::Json => serde_json::to_writer(&mut writer, value)?,
        ResolvedOutputMode::Ndjson => match value {
            Value::Object(map) if map.get("records").is_some_and(Value::is_array) => {
                if let Some(records) = map.get("records").and_then(Value::as_array) {
                    for item in records {
                        serde_json::to_writer(&mut writer, item)?;
                        writer.write_all(b"\n")?;
                    }
                }
                return Ok(());
            }
            Value::Array(values) => {
                for item in values {
                    serde_json::to_writer(&mut writer, item)?;
                    writer.write_all(b"\n")?;
                }
                return Ok(());
            }
            value => serde_json::to_writer(&mut writer, value)?,
        },
        ResolvedOutputMode::Table | ResolvedOutputMode::Plain => render_table(&mut writer, value)?,
    }
    if matches!(mode, ResolvedOutputMode::Table | ResolvedOutputMode::Plain) {
        return Ok(());
    }
    writer.write_all(b"\n")
}

fn render_table(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    match value {
        Value::Object(map) if map.get("records").is_some_and(Value::is_array) => {
            if let Some(records) = map.get("records").and_then(Value::as_array) {
                render_records(writer, records)?;
            }
        }
        Value::Array(values) if values.iter().all(is_record) => render_records(writer, values)?,
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

fn is_record(value: &Value) -> bool {
    value.get("id").is_some() && value.get("kind").is_some()
}

fn render_records(writer: &mut impl Write, records: &[Value]) -> io::Result<()> {
    writeln!(writer, "ID\tKIND\tSTATUS\tTITLE\tTAGS\tPATH")?;
    for record in records {
        let metadata = record.get("metadata").and_then(Value::as_object);
        let tags = metadata
            .and_then(|metadata| metadata.get("tags"))
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let title = metadata
            .and_then(|metadata| metadata.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let status = metadata
            .and_then(|metadata| metadata.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}",
            record.get("id").map(scalar).unwrap_or_default(),
            record.get("kind").map(scalar).unwrap_or_default(),
            status,
            title,
            tags,
            record.get("path").map(scalar).unwrap_or_default(),
        )?;
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
