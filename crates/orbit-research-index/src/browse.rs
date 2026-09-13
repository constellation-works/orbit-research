//! Portable static export. Imported markup is text, media is opt-in and digest checked.
//! Ports `src/orbit_research/browser.py`. No HTTP server: this crate only ever writes
//! files, matching `docs/browser.md`'s "ordinary static serving, there is no API".

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use orbit_research_owner::Checkout;
use serde_json::{Value, json};

use crate::build::{key_of, pin};
use crate::config::{guard_output, load_config};
use crate::read::read_index;
use crate::{IndexError, Result, require, text};

const WEB_INDEX_HTML: &[u8] = include_bytes!("../../../src/orbit_research/web/index.html");
const WEB_APP_JS: &[u8] = include_bytes!("../../../src/orbit_research/web/app.js");
const WEB_STYLE_CSS: &[u8] = include_bytes!("../../../src/orbit_research/web/style.css");

#[derive(Debug)]
pub struct ExportOutcome {
    pub output: PathBuf,
    pub records: usize,
    pub content_digest: String,
}

/// Operator mapping into a local static server, never remote or active schemes.
pub fn local_url(value: &str) -> Result<String> {
    require(
        value.starts_with('/') && !value.starts_with("//"),
        "checkout URL must be a local absolute URL path",
    )?;
    require(
        !value.contains(['\\', '%', '?', '#', ':']) && !value.split('/').any(|part| part == ".."),
        "unsafe checkout URL",
    )?;
    Ok(format!("{}/", value.trim_end_matches('/')))
}

pub fn export_browser(database: &Path, output: &Path, config_path: &Path) -> Result<ExportOutcome> {
    let config = load_config(config_path)?;
    let mut inputs = vec![database.to_path_buf(), config_path.to_path_buf()];
    inputs.extend(config.paths.iter().cloned());
    let output = guard_output(output, &config.roots, &inputs)?;
    require(
        !output.exists(),
        "static export destination must be new; retain the previous usable export",
    )?;
    let mut projection = read_index(database)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".research-browser-")
        .tempdir_in(parent)?;

    let nodes: HashSet<String> = projection
        .get("records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|record| record.get("key").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let media = media_assets(&config.raw, &config.roots, stage.path(), &nodes);
    projection["media"] = json!(media);

    let canonical = orbit_research_contract::canonical_json(&projection)?;
    // JSON is inert even if it contains </script>, quotes or malicious Markdown.
    let mut index_json = canonical.clone();
    index_json.push(b'\n');
    std::fs::write(stage.path().join("index.json"), &index_json)?;
    let encoded = base64_encode(&canonical);
    let data_js = format!(
        "window.RESEARCH_DATA = JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(\"{encoded}\"), c => c.charCodeAt(0))));\n"
    );
    std::fs::write(stage.path().join("data.js"), data_js)?;
    std::fs::write(stage.path().join("index.html"), WEB_INDEX_HTML)?;
    std::fs::write(stage.path().join("app.js"), WEB_APP_JS)?;
    std::fs::write(stage.path().join("style.css"), WEB_STYLE_CSS)?;

    // Assembled in a sibling staging directory and published after every asset exists.
    // `stage`'s directory no longer exists once this succeeds; its `Drop` cleanup then
    // silently no-ops, and on failure it still removes the still-present staging tree.
    std::fs::rename(stage.path(), &output)?;

    let records = projection.get("records").and_then(Value::as_array).map_or(0, Vec::len);
    let content_digest = projection
        .get("content_digest")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(ExportOutcome {
        output,
        records,
        content_digest,
    })
}

/// Explicitly selected, digest-checked media associated with an exact indexed record.
/// Media is never discovered from imported HTML/Markdown.
fn media_assets(
    config: &Value,
    roots: &BTreeMap<String, Checkout>,
    destination: &Path,
    nodes: &HashSet<String>,
) -> Vec<Value> {
    let mut urls: HashMap<String, String> = HashMap::new();
    for (repository, url) in config
        .get("checkout_urls")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(url) = url.as_str()
            && let Ok(mapped) = local_url(url)
        {
            urls.insert(repository.clone(), mapped);
        }
    }
    let mut result = Vec::new();
    for item in config.get("media").and_then(Value::as_array).into_iter().flatten() {
        let label = item.get("label").and_then(Value::as_str).unwrap_or("Artifact");
        let role = item.get("role").and_then(Value::as_str).unwrap_or("illustration");
        let record_pin = item.get("record").cloned().unwrap_or(Value::Null);
        let record_key = key_of(&pin(&record_pin)).unwrap_or_default();
        let mut entry = json!({
            "label": label,
            "role": role,
            "record": record_key,
            "state": "inaccessible",
            "reason": "",
            "url": Value::Null,
            "image": Value::Null,
        });
        if let Err(error) = fill_media_entry(&mut entry, item, nodes, roots, &urls, destination) {
            entry["reason"] = json!(error.to_string());
        }
        result.push(entry);
    }
    result
}

fn fill_media_entry(
    entry: &mut Value,
    item: &Value,
    nodes: &HashSet<String>,
    roots: &BTreeMap<String, Checkout>,
    urls: &HashMap<String, String>,
    destination: &Path,
) -> Result<()> {
    let record_key = entry.get("record").and_then(Value::as_str).unwrap_or_default().to_owned();
    require(nodes.contains(&record_key), "media record pin is absent from index")?;
    let role = entry.get("role").and_then(Value::as_str).unwrap_or_default().to_owned();
    require(
        matches!(role.as_str(), "illustration" | "empirical-evidence" | "simulation"),
        "media role must explicitly distinguish illustration, empirical-evidence or simulation",
    )?;
    // Remote links are visible text only. No request is made at build or page load.
    if let Some(url) = item.get("url").and_then(Value::as_str) {
        require(
            is_credential_free_https(url),
            "only explicit credential-free HTTPS navigation is permitted",
        )?;
        entry["url"] = json!(url);
        entry["state"] = json!("external-unverified");
        entry["reason"] = json!("External content; opens only on request. Not checked or fetched.");
        return Ok(());
    }
    let repository = text(item, "repository").unwrap_or_default();
    let root = roots
        .get(repository)
        .ok_or_else(|| IndexError::Invalid(format!("unrouted checkout: {repository}")))?;
    let item_path = text(item, "path").unwrap_or_default();
    let source_revision = text(item, "source_revision").unwrap_or_default();
    let expected_sha256 = text(item, "sha256").unwrap_or_default();
    let path = root.safe_path(item_path)?;
    let data = root.git_bytes(source_revision, item_path)?;
    require(
        crate::digest_bytes(&data) == expected_sha256,
        "media digest differs from exact Git snapshot",
    )?;
    require(
        path.is_file() && crate::file_digest(&path)? == expected_sha256,
        "media missing or changed in mapped checkout",
    )?;
    // Raster signatures only; HTML, SVG, scripts and polyglot extensions are not embedded.
    let suffix = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_lowercase()))
        .unwrap_or_default();
    let raster = (suffix == ".png" && data.starts_with(b"\x89PNG\r\n\x1a\n"))
        || ((suffix == ".jpg" || suffix == ".jpeg") && data.starts_with(&[0xFF, 0xD8, 0xFF]))
        || (suffix == ".webp" && data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP");
    if raster && role != "simulation" {
        let digest_hex = expected_sha256.strip_prefix("sha256:").unwrap_or(expected_sha256);
        let name = format!("{digest_hex}{suffix}");
        let media_dir = destination.join("media");
        std::fs::create_dir_all(&media_dir)?;
        std::fs::write(media_dir.join(&name), &data)?;
        entry["image"] = json!(format!("media/{name}"));
        entry["url"] = json!(format!("media/{name}"));
        entry["state"] = json!("verified-snapshot");
        entry["reason"] = json!(
            "Exact bytes verified; media role is an owner/operator assertion, not scientific adjudication."
        );
    } else if let Some(base) = urls.get(repository) {
        entry["url"] = json!(format!("{base}{}", quote_path(item_path)));
        entry["state"] = json!("local-navigation");
        entry["reason"] = json!(
            "Explicit navigation to mapped checkout. Bytes verified at export; rebuild after changes. Imported scripts never run in this browser."
        );
    } else {
        return Err(IndexError::Invalid(
            "non-raster artifact requires an explicit checkout_urls mapping for navigation".into(),
        ));
    }
    Ok(())
}

/// `https://` with a non-empty host and no embedded `user[:pass]@` credentials.
fn is_credential_free_https(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    !authority.is_empty() && !authority.contains('@')
}

/// Match CPython `urllib.parse.quote(value, safe="/")`.
fn quote_path(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}
