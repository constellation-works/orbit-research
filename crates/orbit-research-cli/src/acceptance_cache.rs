//! Positive-acceptance cache for the `awaiting-acceptance` panel.
//!
//! A stored `research-acceptance.json` never changes (`accept` refuses to
//! overwrite it), so once the panel has seen one for (research id, task, README
//! blob) it need not ask Orbit again on every refresh. Entries live under the
//! plugin's own writable state directory, `_data/orbit-research-operations`,
//! which the manifest grants for writing and Git ignores.
//!
//! The cache can only ever remove a row, so it must never invent an acceptance:
//!
//! - an entry is written only after a live read showed the artifact naming this
//!   research id and this exact blob;
//! - the key includes the blob, so a result edited after acceptance misses;
//! - a read trusts the file's content, not its name: research id, task and blob
//!   must all match exactly, and anything unreadable, oversized or oddly
//!   shaped is a miss;
//! - it is used only inside a prepared state directory (the `.layout` marker
//!   written by `workspace prepare-operations`), so the panel never creates
//!   that directory and cannot make a later preparation refuse a non-empty
//!   target. It never follows a symlink;
//! - every failure, including a sandbox that denies the write, falls back to
//!   the live query.
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const STATE_PATH: &str = "_data/orbit-research-operations";
const MARKER: &str = ".layout";
const DIRECTORY: &str = "acceptance";
/// Entries are a few dozen bytes; refuse to read anything much larger.
const MAX_ENTRY_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Entry {
    research_id: String,
    task: String,
    blob: String,
}

/// The cache directory of a prepared workspace, or `None` when the state
/// directory is missing, unprepared or reached through a symlink.
fn directory(workspace: &Path) -> Option<PathBuf> {
    let mut path = workspace.to_owned();
    for part in STATE_PATH.split('/') {
        path.push(part);
        if !fs::symlink_metadata(&path).ok()?.is_dir() {
            return None;
        }
    }
    if !fs::symlink_metadata(path.join(MARKER)).ok()?.is_file() {
        return None;
    }
    Some(path.join(DIRECTORY))
}

fn file_name(entry: &Entry) -> Option<String> {
    let id = &entry.research_id;
    let valid_id =
        id.len() == 4 && id.starts_with('R') && id[1..].bytes().all(|b| b.is_ascii_digit());
    let valid_blob =
        (40..=64).contains(&entry.blob.len()) && entry.blob.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid_id || !valid_blob || entry.task.is_empty() || entry.task.len() > 100 {
        return None;
    }
    let task: String = entry
        .task
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(format!("{id}-{task}-{}.json", entry.blob))
}

/// Whether acceptance of (`research_id`, `task`, `blob`) was recorded earlier.
pub(crate) fn is_accepted(workspace: &Path, research_id: &str, task: &str, blob: &str) -> bool {
    let wanted = Entry {
        research_id: research_id.into(),
        task: task.into(),
        blob: blob.into(),
    };
    let (Some(directory), Some(name)) = (directory(workspace), file_name(&wanted)) else {
        return false;
    };
    let path = directory.join(name);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > MAX_ENTRY_BYTES {
        return false;
    }
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        .is_some_and(|stored| stored == wanted)
}

/// Record an acceptance a live read just confirmed. Best effort and
/// write-once: an existing entry is never replaced.
pub(crate) fn record(workspace: &Path, research_id: &str, task: &str, blob: &str) {
    let entry = Entry {
        research_id: research_id.into(),
        task: task.into(),
        blob: blob.into(),
    };
    let (Some(directory), Some(name)) = (directory(workspace), file_name(&entry)) else {
        return;
    };
    let _ = store(&directory, &name, &entry);
}

fn store(directory: &Path, name: &str, entry: &Entry) -> std::io::Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(directory)?,
        Err(error) => return Err(error),
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&serde_json::to_vec(entry)?)?;
    temporary.as_file().sync_all()?;
    // `persist_noclobber` leaves an existing entry untouched.
    match temporary.persist_noclobber(directory.join(name)) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error),
    }
}
