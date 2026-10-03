//! Human-readable `key: value` rendering for single results (a record, a task
//! draft, a write receipt, correlations). Machine formats never pass through here.
use serde_json::{Map, Value};
use std::io::{self, Write};

/// Longest scalar list that is joined on one line (for example tags).
const INLINE_LIST_WIDTH: usize = 72;

/// Metadata keys shown first, in this order, after the record's identity.
const METADATA_ORDER: [&str; 13] = [
    "title",
    "status",
    "tags",
    "derived_from",
    "answered_by",
    "tests",
    "claims",
    "supersedes",
    "orbit",
    "revision",
    "created",
    "updated",
    "slug",
];

/// A record as returned by `research show`: identity and metadata first, the
/// Markdown body last so a long body never pushes the metadata off screen.
pub(super) fn is_record(value: &Value) -> bool {
    ["id", "kind", "path", "body"]
        .iter()
        .all(|key| value.get(key).is_some_and(Value::is_string))
        && value.get("metadata").is_some_and(Value::is_object)
}

pub(super) fn record(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let metadata = value["metadata"].as_object().cloned().unwrap_or_default();
    for key in ["id", "kind"] {
        detail(out, &value[key], key)?;
    }
    for key in METADATA_ORDER.iter().take(2) {
        if let Some(item) = metadata.get(*key) {
            detail(out, item, key)?;
        }
    }
    detail(out, &value["path"], "path")?;
    for key in METADATA_ORDER.iter().skip(2) {
        if let Some(item) = metadata.get(*key) {
            detail(out, item, key)?;
        }
    }
    for (key, item) in &metadata {
        if !METADATA_ORDER.contains(&key.as_str()) && !matches!(key.as_str(), "id" | "assessments")
        {
            detail(out, item, key)?;
        }
    }
    if let Some(Value::Array(assessments)) = metadata.get("assessments") {
        assessment_list(out, assessments)?;
    }
    for key in ["git_blob", "content_sha256"] {
        if let Some(item) = value.get(key) {
            detail(out, item, key)?;
        }
    }
    let body = value["body"].as_str().unwrap_or_default().trim();
    if !body.is_empty() {
        writeln!(out, "\nbody:\n{}", safe_text(body))?;
    }
    Ok(())
}

/// Numbered, one entry per verdict, so a note maps to the assessment it belongs to.
fn assessment_list(out: &mut impl Write, assessments: &[Value]) -> io::Result<()> {
    if assessments.is_empty() {
        return writeln!(out, "assessments: -");
    }
    writeln!(out, "assessments:")?;
    for (index, entry) in assessments.iter().enumerate() {
        let text = |key: &str| entry.get(key).map_or_else(|| "-".into(), scalar);
        writeln!(
            out,
            "  [{}] {} {} revision {} {} ({})",
            index + 1,
            text("date"),
            text("research"),
            text("revision"),
            text("verdict"),
            text("strength"),
        )?;
        if let Some(note) = entry.get("note") {
            writeln!(out, "      note: {}", scalar(note))?;
        }
    }
    Ok(())
}

/// Generic projection. Objects print one `key: value` line per field;
/// multi-line text is indented under its key; lists of structured items are
/// numbered `key[1]`, and top-level items are separated by a blank line.
pub(super) fn detail(out: &mut impl Write, value: &Value, prefix: &str) -> io::Result<()> {
    match value {
        Value::Object(map) => object(out, map, prefix),
        Value::Array(values) => list(out, values, prefix),
        Value::String(text) if text.contains('\n') => {
            if prefix.is_empty() {
                writeln!(out, "{}", safe_text(text))
            } else {
                writeln!(out, "{prefix}:")?;
                for line in text.lines() {
                    if line.is_empty() {
                        writeln!(out)?;
                    } else {
                        writeln!(out, "  {}", safe_text(line))?;
                    }
                }
                Ok(())
            }
        }
        value => {
            let text = scalar(value);
            if prefix.is_empty() {
                writeln!(out, "{text}")
            } else {
                writeln!(out, "{prefix}: {text}")
            }
        }
    }
}

fn object(out: &mut impl Write, map: &Map<String, Value>, prefix: &str) -> io::Result<()> {
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

fn list(out: &mut impl Write, values: &[Value], prefix: &str) -> io::Result<()> {
    if values.is_empty() {
        return if prefix.is_empty() {
            writeln!(out, "-")
        } else {
            writeln!(out, "{prefix}: -")
        };
    }
    if !prefix.is_empty() && values.iter().all(is_inline) {
        let items: Vec<String> = values.iter().map(scalar).collect();
        let joined = items.join(", ");
        if items.iter().all(|item| !item.contains(','))
            && joined.chars().count() <= INLINE_LIST_WIDTH
        {
            return writeln!(out, "{prefix}: {joined}");
        }
    }
    for (index, value) in values.iter().enumerate() {
        if prefix.is_empty() {
            if index > 0 {
                writeln!(out)?;
            }
            detail(out, value, "")?;
        } else {
            detail(out, value, &format!("{prefix}[{}]", index + 1))?;
        }
    }
    Ok(())
}

fn is_inline(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.contains('\n'),
        Value::Object(_) | Value::Array(_) => false,
        _ => true,
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => safe_text(text),
        Value::Null => "-".into(),
        value => value.to_string(),
    }
}

/// Escape control characters other than newline and tab so record text cannot
/// drive the terminal.
pub(super) fn safe_text(text: &str) -> String {
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
