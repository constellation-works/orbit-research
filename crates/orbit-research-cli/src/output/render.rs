//! Shared projections of application values; commands never choose presentation.
use super::sink::{Mode, OutputSink};
use super::table::render_records;
use serde_json::{Value, json};
use std::io::{self, Write};

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

pub(crate) fn render(
    out: &mut impl Write,
    diagnostics: &mut impl Write,
    value: &Value,
    sink: &OutputSink,
) -> io::Result<()> {
    match sink.mode {
        Mode::Json => {
            if sink.interactive {
                serde_json::to_writer_pretty(&mut *out, value)?;
            } else {
                serde_json::to_writer(&mut *out, value)?;
            }
            writeln!(out)
        }
        Mode::Ndjson => {
            if let Some(records) = records(value).or_else(|| value.as_array()) {
                for record in records {
                    json_line(out, record)?;
                }
                Ok(())
            } else {
                json_line(out, value)
            }
        }
        Mode::Table | Mode::Plain => {
            if let Some(records) = records(value) {
                render_records(out, diagnostics, records, sink)
            } else if let Some((valid, revision, record_count, tag_count)) =
                validation_summary(value)
            {
                let outcome = if valid { "passed" } else { "failed" };
                writeln!(
                    out,
                    "Corpus validation {outcome} at base revision {}: {record_count} record{}, {tag_count} tag{}.",
                    safe_text(revision),
                    if record_count == 1 { "" } else { "s" },
                    if tag_count == 1 { "" } else { "s" },
                )
            } else if let Some(skill) = value.get("skill").and_then(Value::as_str) {
                writeln!(out, "{}", safe_text(skill))
            } else {
                detail(out, value, "")
            }
        }
    }
}

fn records(value: &Value) -> Option<&Vec<Value>> {
    value.get("records").and_then(Value::as_array).or_else(|| {
        value.as_array().filter(|rows| {
            rows.iter()
                .all(|row| row.get("id").is_some() && row.get("kind").is_some())
        })
    })
}

fn validation_summary(value: &Value) -> Option<(bool, &str, u64, u64)> {
    Some((
        value.get("valid")?.as_bool()?,
        value.get("base_revision")?.as_str()?,
        value.get("record_count")?.as_u64()?,
        value.get("tag_count")?.as_u64()?,
    ))
}

fn json_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    writeln!(out)?;
    out.flush()
}

fn detail(out: &mut impl Write, value: &Value, prefix: &str) -> io::Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let label = if prefix.is_empty() {
                    safe_text(key)
                } else {
                    format!("{prefix}.{}", safe_text(key))
                };
                detail(out, value, &label)?;
            }
            Ok(())
        }
        Value::Array(values) => {
            if values.is_empty() {
                writeln!(out, "{prefix}: -")?;
            }
            for value in values {
                detail(out, value, prefix)?;
            }
            Ok(())
        }
        value => {
            let text = match value {
                Value::String(s) => safe_text(s),
                Value::Null => "-".into(),
                _ => value.to_string(),
            };
            if prefix.is_empty() {
                writeln!(out, "{text}")
            } else {
                writeln!(out, "{prefix}: {text}")
            }
        }
    }
}

fn safe_text(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

pub(crate) fn render_error(
    out: &mut impl Write,
    error: &Invalid,
    sink: &OutputSink,
) -> io::Result<()> {
    if sink.machine() {
        let mut payload = json!({"error":{"code":"invalid-input","message":error.message}});
        if let Some(problems) = &error.problems {
            payload["error"]["problems"] = json!(problems);
        }
        json_line(out, &payload)
    } else {
        if error.message.starts_with("error:") {
            writeln!(out, "{}", safe_text(&error.message))?;
        } else {
            writeln!(out, "error: {}", safe_text(&error.message))?;
        }
        if let Some(problems) = &error.problems {
            for problem in problems {
                detail(out, problem, "")?;
            }
        }
        Ok(())
    }
}
