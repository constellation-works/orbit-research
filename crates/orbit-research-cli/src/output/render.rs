//! Shared projections of application values; commands never choose presentation.
use super::detail::{self, detail, safe_text};
use super::sink::{Mode, OutputSink};
use super::table::render_records;
use serde_json::{Value, json};
use std::io::{self, Write};

pub(crate) struct Invalid {
    pub(crate) message: String,
    /// Structured problems for machine output (`path`, `field`, `message`
    /// objects for a corpus that fails validation). The text form already lists
    /// them in `message`, so it does not repeat them.
    pub(crate) problems: Option<Vec<Value>>,
}

impl From<&orbit_research_core::Error> for Invalid {
    fn from(error: &orbit_research_core::Error) -> Self {
        let problems = match error {
            orbit_research_core::Error::Corpus(issues) => Some(
                issues
                    .iter()
                    .map(|issue| json!(issue))
                    .collect::<Vec<Value>>(),
            ),
            _ => None,
        };
        Self {
            message: error.to_string(),
            problems,
        }
    }
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
                // The packaged text already ends with a newline.
                write!(out, "{}", safe_text(skill.trim_end_matches('\n')))?;
                writeln!(out)
            } else if detail::is_record(value) {
                detail::record(out, value)
            } else if value.as_array().is_some_and(Vec::is_empty) {
                // Only `work-links` returns a bare list; the other empty cases
                // are objects.
                writeln!(diagnostics, "No work links found.")
            } else {
                detail(out, value, "")
            }
        }
    }
}

fn records(value: &Value) -> Option<&Vec<Value>> {
    value.get("records").and_then(Value::as_array).or_else(|| {
        value.as_array().filter(|rows| {
            !rows.is_empty()
                && rows
                    .iter()
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
        Ok(())
    }
}
