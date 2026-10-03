//! Shared projections of application values; commands never choose presentation.
use super::detail::{self, detail, safe_text};
use super::sink::{Mode, OutputSink};
use super::table::render_records;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::path::Path;

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

impl Invalid {
    /// An application error for a command that was given `corpus`, so its next
    /// step can name the corpus: the check to rerun for a long problem list,
    /// the listing that shows which ids exist for an unknown one.
    pub(crate) fn for_corpus(error: &orbit_research_core::Error, corpus: &Path) -> Self {
        use orbit_research_core::Error;
        let mut invalid = Self::from(error);
        let corpus = shell_word(corpus);
        match error {
            Error::Corpus(issues) => {
                let rerun = format!("orbit-research research check --corpus {corpus}");
                invalid.message = orbit_research_core::render_issues_with(issues, &rerun);
            }
            Error::NotFound(message) => {
                invalid.message = format!(
                    "{message}; run `orbit-research research list --corpus {corpus}` to see the ids that exist"
                );
            }
            _ => (),
        }
        invalid
    }
}

/// A path as one shell word: bare when it is plain, single-quoted otherwise.
fn shell_word(path: &Path) -> String {
    let text = path.display().to_string();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./~:+=@%,".contains(c));
    if plain {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
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
            } else if let Some(id) = unchanged_write(value) {
                writeln!(
                    out,
                    "No changes: {} already has these values.",
                    safe_text(id)
                )?;
                let mut receipt = value.clone();
                if let Some(map) = receipt.as_object_mut() {
                    map.remove("changed");
                }
                detail(out, &receipt, "")
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

/// The record a write receipt names when the write left it as it was
/// (`changed: false`); every receipt that changed something omits the field.
fn unchanged_write(value: &Value) -> Option<&str> {
    if value.get("changed") == Some(&Value::Bool(false)) {
        value.get("id").and_then(Value::as_str)
    } else {
        None
    }
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
