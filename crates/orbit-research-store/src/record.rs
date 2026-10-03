//! Canonical Markdown encoding and filename rules.
use crate::{Error, Result};
use orbit_research_common::CorpusIssue;
use serde_json::Value;

/// Split a record file into its frontmatter and body. A failure names `path`
/// and says what the file must look like.
pub(crate) fn parse(path: &str, text: &str) -> Result<(Value, String)> {
    let problem = |message: String| {
        Error::Corpus(vec![CorpusIssue {
            path: path.to_owned(),
            field: None,
            message,
        }])
    };
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n").ok_or_else(|| {
        problem(
            "missing frontmatter: the file must start with a line holding only `---`, then YAML fields, then a closing `---` line"
                .into(),
        )
    })?;
    let (front, body) = rest.split_once("\n---\n").ok_or_else(|| {
        problem("unclosed frontmatter: add a line holding only `---` after the YAML fields".into())
    })?;
    let metadata = serde_yaml::from_str(front).map_err(|error| {
        // The YAML sits below the opening `---`, so its line numbers are one short.
        problem(format!(
            "frontmatter is not valid YAML: {}",
            shift_lines(&error.to_string(), 1)
        ))
    })?;
    Ok((metadata, body.to_owned()))
}

/// Add `by` to every `line N` in a parser message.
fn shift_lines(message: &str, by: usize) -> String {
    let mut shifted = String::new();
    let mut rest = message;
    while let Some(at) = rest.find("line ") {
        let (head, tail) = rest.split_at(at + "line ".len());
        shifted.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        match tail[..digits].parse::<usize>() {
            Ok(line) => shifted.push_str(&(line + by).to_string()),
            Err(_) => shifted.push_str(&tail[..digits]),
        }
        rest = &tail[digits..];
    }
    shifted.push_str(rest);
    shifted
}

/// Split `Q001-short-slug.md` (or the directory `R001-short-slug`) into its id
/// and slug. The error is a plain sentence for the caller to attach a path to.
pub(crate) fn parse_record_name(
    kind: &str,
    name: &str,
    directory_layout: bool,
) -> std::result::Result<(String, String), String> {
    let expected = if directory_layout {
        format!("{kind}001-short-slug/ (a directory holding README.md)")
    } else {
        format!("{kind}001-short-slug.md")
    };
    let wrong = || {
        format!(
            "the name `{name}` is not a record name; use {expected}, with a three-digit id and a lowercase slug of letters, digits and single hyphens"
        )
    };
    let suffix = if directory_layout {
        name
    } else {
        name.strip_suffix(".md").ok_or_else(wrong)?
    };
    let (id, slug) = suffix.split_once('-').ok_or_else(wrong)?;
    if id.len() != kind.len() + 3
        || !id.starts_with(kind)
        || !id[kind.len()..].bytes().all(|byte| byte.is_ascii_digit())
        || slug.is_empty()
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(wrong());
    }
    Ok((id.to_owned(), slug.to_owned()))
}

pub(crate) fn kebab(title: &str) -> String {
    let mut slug = String::new();
    for byte in title.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            slug.push(byte as char);
        } else if byte.is_ascii_uppercase() {
            slug.push((byte + b'a' - b'A') as char);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// The path slug for a new record. Non-ASCII titles are transliterated
/// (`Café` becomes `cafe`, CJK text becomes its romanization) so they keep
/// their meaning, and a title with nothing pronounceable falls back to
/// `untitled`. The result always matches the contract's `slug_pattern`.
pub(crate) fn slug_for(title: &str) -> String {
    let slug = kebab(&deunicode::deunicode(title));
    if slug.is_empty() {
        UNTITLED_SLUG.to_owned()
    } else {
        slug
    }
}

const UNTITLED_SLUG: &str = "untitled";

/// `raw` is everything after the closing delimiter line, kept byte for byte.
pub(crate) fn render(metadata: &Value, raw: &str) -> Result<String> {
    Ok(format!(
        "---\n{}---\n{raw}",
        serde_yaml::to_string(metadata)?
    ))
}

pub(crate) fn utc_date() -> Result<String> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Invalid("Cannot read UTC date".into()))?;
    let (year, month, day) = civil_from_days((elapsed.as_secs() / 86_400) as i64);
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

/// Days since the Unix epoch to a proleptic-Gregorian (year, month, day),
/// per Howard Hinnant's `civil_from_days`: <https://howardhinnant.github.io/date_algorithms.html>.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// A reserved R stub's Method, Result, Limitations and Next text. Delivery
/// validation treats a section still holding one of these as unwritten.
pub(crate) const RESEARCH_PLACEHOLDERS: [&str; 4] =
    ["Pending.", "Pending.", "Not yet run.", "Await dispatch."];

/// The status every new record of `kind` starts in.
pub fn initial_status(kind: &str) -> &'static str {
    match kind {
        "R" => "planned",
        "T" => "active",
        _ => "open",
    }
}

pub(crate) fn scaffold(
    contract: &crate::validation::Contract,
    id: &str,
    kind: &str,
    title: &str,
    body: &str,
    tags: Vec<String>,
    derived_from: Vec<String>,
) -> Result<(String, String)> {
    use serde_json::json;
    let full_slug = slug_for(title);
    let slug = capped_slug(&full_slug);
    let date = utc_date()?;
    let mut meta = json!({
        "id": id,
        "title": title,
        "status": initial_status(kind),
        "tags": tags,
        "derived_from": derived_from,
        "created": date,
        "updated": date,
    });
    match kind {
        "Q" => {
            meta["answered_by"] = json!([]);
        }
        "H" => {
            meta["revision"] = json!(1);
            meta["assessments"] = json!([]);
        }
        "T" => {
            meta["claims"] = json!([]);
            meta["supersedes"] = json!([]);
        }
        _ => {
            meta["tests"] = json!([]);
        }
    }
    if slug != kebab(title) {
        // The path slug is not the plain kebab-case of the title (a long captured
        // line was shortened, or the title was transliterated or has no ASCII
        // letters). Declare it so validation, which compares a slug-less record's
        // path to the kebab-case of its title, accepts it.
        meta["slug"] = json!(slug);
    }
    contract.validate_input(&meta, id, kind)?;
    let directory = contract.schema["x-observatory"]["kinds"][kind]["directory"]
        .as_str()
        .ok_or_else(|| Error::Invalid("Missing owner directory".into()))?;
    // snapshot() has already validated every existing owner directory for traversal/symlinks.
    let path = if kind != "R" {
        format!("{directory}/{id}-{slug}.md")
    } else {
        format!("{directory}/{id}-{slug}/README.md")
    };
    let body = if kind == "Q" {
        format!("# {id} — {title}\n\n## The question\n\n{body}\n")
    } else if kind == "H" {
        format!(
            "# {id} — {title}\n\n## The claim\n\n{body}\n\n## What would refute it\n\nTo be specified before testing.\n"
        )
    } else if kind == "T" {
        format!(
            "# {id} — {title}\n\n## What it says\n\n{body}\n\n## Where it stops\n\nScope and limitations remain to be specified.\n"
        )
    } else {
        let [method, result, limitations, next] = RESEARCH_PLACEHOLDERS;
        format!(
            "# {id} — {title}\n\n## Question\n\n{body}\n\n## Method\n\n{method}\n\n## Result\n\n{result}\n\n## Limitations\n\n{limitations}\n\n## Next\n\n{next}\n"
        )
    };
    let text = render(&meta, &format!("\n{body}"))?;
    Ok((path, text))
}

/// Keep record paths readable: cut long slugs at a word boundary.
fn capped_slug(slug: &str) -> &str {
    const MAX: usize = 60;
    if slug.len() <= MAX {
        return slug;
    }
    let head = &slug[..MAX];
    head.rsplit_once('-')
        .map_or(head, |(words, _)| words)
        .trim_end_matches('-')
}
