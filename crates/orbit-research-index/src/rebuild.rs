//! Atomic SQLite rebuild: lock the output directory, rebuild the projection under that
//! lock, then publish a complete replacement file with a rename. Ports `index.py::rebuild`.
//!
//! Invalid JSON, bad digests/schema, or an error/interruption before the final rename
//! preserves the previous database untouched: nothing about `output` is opened until a
//! complete, valid projection is ready to write.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use crate::build::build_projection;
use crate::config::{guard_output, load_config};
use crate::{IndexError, Result};

#[derive(Debug)]
pub struct RebuildOutcome {
    pub database: PathBuf,
    pub records: usize,
    pub content_digest: String,
    pub pending: usize,
}

pub fn rebuild(config_path: &Path, database: &Path) -> Result<RebuildOutcome> {
    let initial = load_config(config_path)?;
    let output = guard_output(database, &initial.roots, &inputs_of(config_path, &initial.paths))?;
    let parent = output.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    std::fs::create_dir_all(&parent)?;
    let lock = DirectoryLock::acquire(&parent)?;

    let (projection, config) = build_projection(config_path)?;
    let output = guard_output(&output, &config.roots, &inputs_of(config_path, &config.paths))?;

    let staged = tempfile::Builder::new()
        .prefix(".research-index-")
        .tempfile_in(&parent)?;
    write_database(staged.path(), &projection.value)?;
    staged.as_file().sync_all()?;
    let temp_path = staged.into_temp_path();
    temp_path
        .persist(&output)
        .map_err(|error| IndexError::Io(error.error))?;

    let directory = std::fs::File::open(&parent)?;
    directory.sync_all()?;
    drop(lock);

    let records_array = projection.value.get("records").and_then(Value::as_array);
    let records = records_array.map_or(0, Vec::len);
    let pending = records_array
        .into_iter()
        .flatten()
        .filter(|record| record.get("reconciliation").and_then(Value::as_str) == Some("pending"))
        .count();
    let content_digest = projection
        .value
        .get("content_digest")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(RebuildOutcome {
        database: output,
        records,
        content_digest,
        pending,
    })
}

fn inputs_of(config_path: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut inputs = vec![config_path.to_path_buf()];
    inputs.extend(paths.iter().cloned());
    inputs
}

fn write_database(path: &Path, projection: &Value) -> Result<()> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE projection (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
         CREATE TABLE records (key TEXT PRIMARY KEY, repository TEXT NOT NULL, id TEXT NOT NULL,
             revision TEXT NOT NULL, source_revision TEXT, kind TEXT NOT NULL, body TEXT NOT NULL);
         CREATE TABLE links (source TEXT NOT NULL, ordinal INTEGER NOT NULL, target TEXT,
             status TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(source, ordinal));
         CREATE INDEX record_identity ON records(repository, id, revision, source_revision);",
    )?;
    let tx = conn.transaction()?;
    {
        let body = canonical_text(projection)?;
        tx.execute("INSERT INTO projection VALUES (1, ?1)", [&body])?;
        for node in projection
            .get("records")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = node.get("key").and_then(Value::as_str).unwrap_or_default();
            let pin = node.get("pin").and_then(Value::as_array).cloned().unwrap_or_default();
            let repository = pin.first().and_then(Value::as_str).unwrap_or_default();
            let id = pin.get(1).and_then(Value::as_str).unwrap_or_default();
            let revision = pin.get(2).and_then(Value::as_str).unwrap_or_default();
            let source_revision = pin.get(3).and_then(Value::as_str);
            let kind = node
                .get("record")
                .and_then(|record| record.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let body = canonical_text(node)?;
            tx.execute(
                "INSERT INTO records VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![key, repository, id, revision, source_revision, kind, body],
            )?;
            for (ordinal, edge) in node
                .get("edges")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let target = edge.get("target").and_then(Value::as_str);
                let status = edge.get("status").and_then(Value::as_str).unwrap_or_default();
                let edge_body = canonical_text(edge)?;
                tx.execute(
                    "INSERT INTO links VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![key, ordinal as i64, target, status, edge_body],
                )?;
            }
        }
    }
    tx.commit()?;
    conn.close().map_err(|(_, error)| IndexError::Sqlite(error))?;
    Ok(())
}

fn canonical_text(value: &Value) -> Result<String> {
    Ok(String::from_utf8_lossy(&orbit_research_contract::canonical_json(value)?).into_owned())
}

/// An exclusive advisory lock on the output directory inode: no mutable database, index or
/// persistent lock file.
struct DirectoryLock {
    descriptor: i32,
}

impl DirectoryLock {
    fn acquire(directory: &Path) -> Result<Self> {
        let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| IndexError::Invalid("output directory path is not usable".into()))?;
        // SAFETY: `path` is a valid NUL-terminated C string for the duration of the call.
        let descriptor = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: `descriptor` is an open file descriptor owned by this value.
        if unsafe { libc::flock(descriptor, libc::LOCK_EX) } != 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: closing the descriptor we just opened.
            unsafe { libc::close(descriptor) };
            return Err(error.into());
        }
        Ok(Self { descriptor })
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // SAFETY: `descriptor` is owned here and closed exactly once; closing releases flock.
        unsafe { libc::close(self.descriptor) };
    }
}
