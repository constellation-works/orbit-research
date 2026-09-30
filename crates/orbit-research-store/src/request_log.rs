//! Local operational request log. Scientific facts never live here.
use crate::{Error, Result, corpus::Corpus};
use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct RequestLog {
    root: PathBuf,
    _lock: File,
}

impl Corpus {
    pub fn request_log(&self) -> Result<RequestLog> {
        let common_dir = crate::git::read::common_git_dir(self.root())?;
        let root = common_dir.join("orbit-research-operations");
        fs::create_dir_all(&root)
            .map_err(|error| io_context("create request-log directory", &root, error))?;
        // The log directory itself may be new. Its parent must be durable too.
        #[cfg(unix)]
        sync_directory(&common_dir, "request-log parent")?;
        let lock_path = root.join("lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| io_context("open request-log lock", &lock_path, error))?;
        lock.lock_exclusive()
            .map_err(|error| io_context("acquire request-log lock", &lock_path, error))?;
        Ok(RequestLog { root, _lock: lock })
    }
}

impl RequestLog {
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Invalid("Invalid operation key".into()));
        }
        Ok(self.root.join(format!("{key}.json")))
    }

    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let path = self.path(key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_context("read request-log entry", &path, error)),
        }
    }

    pub fn save<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let path = self.path(key)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|error| io_context("create request-log temporary file", &self.root, error))?;
        temp.write_all(&serde_json::to_vec_pretty(value)?)
            .map_err(|error| io_context("write request-log temporary file", temp.path(), error))?;
        temp.as_file()
            .sync_all()
            .map_err(|error| io_context("sync request-log temporary file", temp.path(), error))?;
        temp.persist(&path)
            .map_err(|error| io_context("publish request-log entry", &path, error.error))?;
        // A successful save is the boundary before non-idempotent Orbit work.
        // Persisting the file does not sync its new directory entry on Unix.
        #[cfg(unix)]
        sync_directory(&self.root, "request-log")?;
        Ok(())
    }

    pub fn list<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        let mut items = Vec::new();
        for entry in fs::read_dir(&self.root)
            .map_err(|error| io_context("list request-log directory", &self.root, error))?
        {
            let path = entry
                .map_err(|error| io_context("read request-log directory entry", &self.root, error))?
                .path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let bytes = fs::read(&path)
                    .map_err(|error| io_context("read request-log entry", &path, error))?;
                items.push(serde_json::from_slice(&bytes)?);
            }
        }
        Ok(items)
    }
}

/// Keep filesystem errors typed for transport classification while naming the
/// precise operation and path that failed, including denied sandbox accesses.
fn io_context(operation: &str, path: &Path, error: std::io::Error) -> Error {
    Error::Io(std::io::Error::new(
        error.kind(),
        RequestLogIoError {
            message: format!("Unable to {operation} at {}: {error}", path.display()),
            source: error,
        },
    ))
}

#[derive(Debug)]
struct RequestLogIoError {
    message: String,
    source: std::io::Error,
}

impl std::fmt::Display for RequestLogIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RequestLogIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path, description: &str) -> Result<()> {
    let directory = File::open(path).map_err(|error| {
        io_context(
            &format!("open {description} directory for sync"),
            path,
            error,
        )
    })?;
    directory
        .sync_all()
        .map_err(|error| io_context(&format!("sync {description} directory"), path, error))?;
    Ok(())
}
