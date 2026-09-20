use serde_json::Value;
use std::io::{self, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::sink::{Mode, OutputSink};

const HEADERS: [&str; 6] = ["ID", "KIND", "STATUS", "TITLE", "TAGS", "PATH"];
const FLEXIBLE: [usize; 3] = [3, 4, 5];
const MIN_FLEX_WIDTH: usize = 8;

pub(crate) fn render_records(
    out: &mut impl Write,
    diagnostics: &mut impl Write,
    records: &[Value],
    sink: &OutputSink,
) -> io::Result<()> {
    match sink.mode {
        Mode::Plain | Mode::Table if records.is_empty() => {
            writeln!(diagnostics, "No research records found.")
        }
        Mode::Plain => render_plain(out, records),
        Mode::Table => render_table(out, diagnostics, records, sink),
        Mode::Json | Mode::Ndjson => Ok(()),
    }
}

fn render_plain(out: &mut impl Write, records: &[Value]) -> io::Result<()> {
    for record in records {
        writeln!(out, "{}", row(record, "").join("\t"))?;
    }
    Ok(())
}

fn render_table(
    out: &mut impl Write,
    diagnostics: &mut impl Write,
    records: &[Value],
    sink: &OutputSink,
) -> io::Result<()> {
    let rows: Vec<[String; 6]> = records.iter().map(|record| row(record, "-")).collect();
    let mut visible = [true; 6];

    if sink.suppress_uniform {
        for column in [1, 2, 4, 5] {
            visible[column] = rows
                .first()
                .is_none_or(|first| rows.iter().any(|item| item[column] != first[column]));
        }
    }

    let mut widths = natural_widths(&rows);
    if sink.width > 0 {
        shrink_to_width(&mut widths, &mut visible, sink.width, diagnostics)?;
    }

    write_table_row(
        out,
        &HEADERS.map(str::to_owned),
        &visible,
        &widths,
        sink.color,
        true,
    )?;
    for row in rows {
        write_table_row(out, &row, &visible, &widths, false, false)?;
    }
    Ok(())
}

fn row(record: &Value, missing: &str) -> [String; 6] {
    let metadata = record.get("metadata");
    [
        field(record.get("id"), missing),
        field(record.get("kind"), missing),
        field(metadata.and_then(|value| value.get("status")), missing),
        field(metadata.and_then(|value| value.get("title")), missing),
        metadata
            .and_then(|value| value.get("tags"))
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .map(|tag| field(Some(tag), missing))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| missing.to_owned()),
        field(record.get("path"), missing),
    ]
}

fn field(value: Option<&Value>, missing: &str) -> String {
    let value = match value {
        None | Some(Value::Null) => missing.to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
    };
    escape_controls(&value)
}

fn escape_controls(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{{{:x}}}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn natural_widths(rows: &[[String; 6]]) -> [usize; 6] {
    let mut widths = HEADERS.map(UnicodeWidthStr::width);
    for row in rows {
        for (column, value) in row.iter().enumerate() {
            widths[column] = widths[column].max(UnicodeWidthStr::width(value.as_str()));
        }
    }
    widths
}

fn shrink_to_width(
    widths: &mut [usize; 6],
    visible: &mut [bool; 6],
    limit: usize,
    diagnostics: &mut impl Write,
) -> io::Result<()> {
    while table_width(widths, visible) > limit {
        let widest = FLEXIBLE
            .iter()
            .copied()
            .filter(|column| visible[*column] && widths[*column] > MIN_FLEX_WIDTH)
            .max_by_key(|column| widths[*column]);
        let Some(column) = widest else { break };
        widths[column] -= 1;
    }

    for column in FLEXIBLE.into_iter().rev() {
        if table_width(widths, visible) <= limit {
            break;
        }
        if visible[column] {
            visible[column] = false;
            writeln!(
                diagnostics,
                "Dropped {} column to fit terminal width.",
                HEADERS[column]
            )?;
        }
    }
    Ok(())
}

fn table_width(widths: &[usize; 6], visible: &[bool; 6]) -> usize {
    let columns = visible.iter().filter(|visible| **visible).count();
    widths
        .iter()
        .zip(visible)
        .filter_map(|(width, visible)| visible.then_some(*width))
        .sum::<usize>()
        + columns.saturating_sub(1) * 2
}

fn write_table_row(
    out: &mut impl Write,
    row: &[String; 6],
    visible: &[bool; 6],
    widths: &[usize; 6],
    color: bool,
    header: bool,
) -> io::Result<()> {
    let columns: Vec<usize> = (0..6).filter(|column| visible[*column]).collect();
    for (position, column) in columns.iter().copied().enumerate() {
        let value = truncate(&row[column], widths[column], column == 5);
        if header && color {
            write!(out, "\x1b[2m{value}\x1b[0m")?;
        } else {
            write!(out, "{value}")?;
        }
        if position + 1 < columns.len() {
            let padding = widths[column].saturating_sub(UnicodeWidthStr::width(value.as_str()));
            write!(out, "{}  ", " ".repeat(padding))?;
        }
    }
    writeln!(out)
}

fn truncate(value: &str, width: usize, middle: bool) -> String {
    if width == 0 || UnicodeWidthStr::width(value) <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    if middle {
        let left = (width - 1).div_ceil(2);
        let right = width - 1 - left;
        format!(
            "{}…{}",
            take_width(value, left),
            take_width_end(value, right)
        )
    } else {
        format!("{}…", take_width(value, width - 1))
    }
}

fn take_width(value: &str, width: usize) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|character| {
            let next = used + UnicodeWidthChar::width(*character).unwrap_or(0);
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect()
}

fn take_width_end(value: &str, width: usize) -> String {
    let mut used = 0;
    let mut characters: Vec<char> = value
        .chars()
        .rev()
        .take_while(|character| {
            let next = used + UnicodeWidthChar::width(*character).unwrap_or(0);
            if next > width {
                false
            } else {
                used = next;
                true
            }
        })
        .collect();
    characters.reverse();
    characters.into_iter().collect()
}
