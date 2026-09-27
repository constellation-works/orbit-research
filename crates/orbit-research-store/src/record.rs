//! Canonical Markdown encoding and filename rules.
use crate::{Error, Result};
use serde_json::Value;

pub(crate) fn parse(text: &str) -> Result<(Value, String)> {
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| Error::Invalid("Missing frontmatter".into()))?;
    let (front, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| Error::Invalid("Unclosed frontmatter".into()))?;
    Ok((serde_yaml::from_str(front)?, body.to_owned()))
}

pub(crate) fn parse_record_name(
    kind: &str,
    name: &str,
    directory_layout: bool,
) -> Result<(String, String)> {
    let suffix = if directory_layout {
        name
    } else {
        name.strip_suffix(".md")
            .ok_or_else(|| Error::Invalid(format!("Record filename must end in .md: {name}")))?
    };
    let (id, slug) = suffix
        .split_once('-')
        .ok_or_else(|| Error::Invalid(format!("Record path must be {kind}###-slug: {name}")))?;
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
        return Err(Error::Invalid(format!(
            "Record path must be {kind}###-slug: {name}"
        )));
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
    let full_slug = kebab(title);
    if full_slug.is_empty() {
        return Err(Error::Invalid(
            "Title needs at least one ASCII letter or number for its path".into(),
        ));
    }
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
    if slug != full_slug {
        // A captured question's first line can be long; freeze a shorter path slug.
        meta["slug"] = json!(slug);
    }
    contract.validate(&meta, id)?;
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
