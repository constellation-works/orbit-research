//! Explicit, atomic transition from Git metadata to ignored shared state.
use crate::{Error, Result, corpus::Corpus, git::read};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) const STATE_PATH: &str = "_data/orbit-research-operations";
const LEGACY_NAME: &str = "orbit-research-operations";
const LEDGER_NAME: &str = ".layout";
const MARKER: &[u8] =
    b"{\"layout_version\":1,\"state_path\":\"_data/orbit-research-operations\"}\n";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    layout_version: u32,
    state_path: String,
}

pub(crate) struct Location {
    pub(crate) root: PathBuf,
    pub(crate) prepared: bool,
    common: PathBuf,
    primary: PathBuf,
    legacy: PathBuf,
    state: PathBuf,
}

fn paths(corpus: &Corpus) -> Result<Location> {
    let common = read::common_git_dir(corpus.root())?;
    let primary = read::primary_worktree(corpus.root())?;
    // A worktree's .git file is ordinary; a redirected .git symlink is not.
    no_symlinks(&corpus.root().join(".git"))?;
    no_symlinks(&common)?;
    no_symlinks(&primary)?;
    let primary_corpus = Corpus::open(&primary).map_err(|error| {
        Error::Refused(format!(
            "Unable to validate the primary corpus at {}: {error}",
            primary.display()
        ))
    })?;
    if read::common_git_dir(primary_corpus.root())?.canonicalize()? != common.canonicalize()? {
        return Err(Error::Refused(
            "Primary corpus has a different Git identity".into(),
        ));
    }
    let legacy = common.join(LEGACY_NAME);
    let state = primary.join(STATE_PATH);
    no_symlinks(&legacy)?;
    no_symlinks(&state)?;
    Ok(Location {
        root: legacy.clone(),
        prepared: false,
        common,
        primary,
        legacy,
        state,
    })
}

pub(crate) fn location(corpus: &Corpus) -> Result<Location> {
    let mut location = paths(corpus)?;
    match fs::symlink_metadata(&location.legacy) {
        Ok(metadata) if metadata.is_file() => {
            check_marker(&location.legacy)?;
            require_directory(&location.state)?;
            check_marker(&location.state.join(LEDGER_NAME))?;
            location.root = location.state.clone();
            location.prepared = true;
        }
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        _ => {
            return Err(Error::Refused(format!(
                "Request storage is not an ordinary directory or layout marker at {}",
                location.legacy.display()
            )));
        }
    }
    Ok(location)
}

pub(crate) fn require_prepared(corpus: &Corpus) -> Result<()> {
    let location = location(corpus)?;
    if location.prepared {
        return Ok(());
    }
    Err(Error::Refused(format!(
        "Shared request storage is not prepared; run `orbit-research workspace prepare-operations {}` on the primary checkout before linking through the plugin",
        location.primary.display()
    )))
}

pub(crate) fn prepare(path: &Path) -> Result<Value> {
    prepare_with_exchange(path, exchange)
}

/// The exchange seam lets tests exercise unsupported filesystems and readers
/// waiting on the old lock at the real commit boundary.
pub(crate) fn prepare_with_exchange(
    path: &Path,
    exchange_entries: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<Value> {
    let corpus = Corpus::open(path)?;
    let initial = paths(&corpus)?;
    if corpus.root() != initial.primary {
        return Err(Error::Refused(format!(
            "Prepare operations on the primary checkout: orbit-research workspace prepare-operations {}",
            initial.primary.display()
        )));
    }
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err(Error::Refused(
            "Atomic request-storage preparation is supported on Linux and macOS; the existing legacy log is preserved".into(),
        ));
    }
    if !corpus.is_ignored(&format!("{STATE_PATH}/probe.json"))? {
        return Err(Error::Refused(format!(
            "Operational state must be ignored by Git; add `/{STATE_PATH}/` to the corpus .gitignore, preserve and commit that owner change, then rerun workspace prepare-operations"
        )));
    }
    if corpus.has_indexed_paths(STATE_PATH)? {
        return Err(Error::Refused("Request-storage target is tracked in the Git index; preserve and untrack those paths before preparing operations".into()));
    }
    for name in ["lock", LEDGER_NAME] {
        if !corpus.is_ignored(&format!("{STATE_PATH}/{name}"))? {
            return Err(Error::Refused(format!(
                "Operational entry {STATE_PATH}/{name} must be ignored by Git before preparing operations"
            )));
        }
    }
    let initial = location(&corpus)?;
    if initial.prepared {
        let _lock = locked(&initial.root.join("lock"))?;
        let current = location(&corpus)?;
        if !current.prepared {
            return Err(Error::Refused(
                "Request-storage layout changed while locking".into(),
            ));
        }
        sync_directory(&current.root)?;
        sync_directory(
            current
                .state
                .parent()
                .ok_or_else(|| Error::Internal("Missing state parent".into()))?,
        )?;
        sync_directory(&current.common)?;
        return Ok(receipt(&current, false));
    }
    check_destination(&initial.state)?;
    if initial.legacy.exists() {
        require_directory(&initial.legacy)?;
        check_entries(&initial.legacy)?;
        check_ignored_entries(&corpus, &initial.legacy)?;
    }
    // Refuse cross-device transitions before adding a ledger or staging file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut ancestor = initial
            .state
            .parent()
            .ok_or_else(|| Error::Internal("Missing state parent".into()))?;
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| Error::Internal("Missing state ancestor".into()))?;
        }
        if fs::metadata(&initial.common)?.dev() != fs::metadata(ancestor)?.dev() {
            return Err(Error::Refused("Cannot atomically prepare request storage across filesystems; the original log is preserved".into()));
        }
    }
    create_directory(&initial.legacy)?;
    let lock = locked(&initial.legacy.join("lock"))?;
    // Another preparer may have exchanged the directory while this descriptor
    // waited. It is still the same lock inode, now in the prepared directory.
    let current = location(&corpus)?;
    if current.prepared {
        sync_directory(&current.root)?;
        sync_directory(&current.common)?;
        sync_directory(
            current
                .state
                .parent()
                .ok_or_else(|| Error::Internal("Missing state parent".into()))?,
        )?;
        return Ok(receipt(&current, false));
    }
    require_directory(&current.legacy)?;
    check_entries(&current.legacy)?;
    check_ignored_entries(&corpus, &current.legacy)?;
    check_destination(&current.state)?;
    if corpus.has_indexed_paths(STATE_PATH)? {
        return Err(Error::Refused("Request-storage target became tracked in the Git index; preserve it for reconciliation".into()));
    }
    if !same_file(&lock, &current.legacy.join("lock"))? {
        return Err(Error::Refused(
            "Request-storage lock changed during preparation".into(),
        ));
    }
    let parent = current
        .state
        .parent()
        .ok_or_else(|| Error::Internal("Missing state parent".into()))?;
    create_directory(parent)?;
    if current.state.is_dir() {
        // Only an empty, verified directory materialized by the host is accepted.
        // A populated target is never removed or combined with the source log.
        fs::remove_dir(&current.state)?;
        sync_directory(parent)?;
    }
    let mut ledger = marker(&current.legacy.join(LEDGER_NAME))?;
    let mut stage = marker(&current.state)?;
    sync_directory(&current.legacy)?;
    sync_directory(parent)?;
    sync_directory(&current.common)?;
    exchange_entries(&current.legacy, &current.state)?;
    // Publication is the commit decision. Neither guard may remove a path after
    // the swap, including when a subsequent directory sync fails.
    ledger.armed = false;
    stage.armed = false;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&current.state, fs::Permissions::from_mode(0o700))?;
    }
    sync_directory(&current.state)?;
    sync_directory(parent)?;
    sync_directory(&current.common)?;
    let prepared = location(&corpus)?;
    Ok(receipt(&prepared, true))
}

fn receipt(location: &Location, changed: bool) -> Value {
    json!({"corpus":location.primary, "prepared":true, "changed":changed,
        "layout_version":1, "state_path":location.state})
}

fn check_destination(path: &Path) -> Result<()> {
    no_symlinks(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => check_marker(path),
        Ok(metadata) if metadata.is_dir() => {
            if fs::read_dir(path)?.next().is_some() {
                return Err(Error::Refused(format!(
                    "Request-storage target {} is not empty; preserve it for reconciliation",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        _ => Err(Error::Refused(format!(
            "Unsafe request-storage target at {}",
            path.display()
        ))),
    }
}

fn check_entries(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Refused(format!(
                "Unsafe request-log entry at {}; preserve it for reconciliation",
                entry.path().display()
            )));
        }
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            Error::Refused("Non-UTF-8 request-log filename; preserve the original state".into())
        })?;
        let keyed = name
            .strip_suffix(".json")
            .is_some_and(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if name != "lock" && name != LEDGER_NAME && !keyed {
            return Err(Error::Refused(format!(
                "Unrecognized request-log entry at {}; preserve it for reconciliation",
                entry.path().display()
            )));
        }
        if name == LEDGER_NAME {
            check_marker(&entry.path())?;
        }
    }
    Ok(())
}

fn check_ignored_entries(corpus: &Corpus, root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let relative = format!("{STATE_PATH}/{}", entry.file_name().to_string_lossy());
        if !corpus.is_ignored(&relative)? {
            return Err(Error::Refused(format!(
                "Operational entry {relative} is not ignored by Git; preserve the legacy log and update the owner ignore policy before preparing operations"
            )));
        }
    }
    Ok(())
}

fn check_marker(path: &Path) -> Result<()> {
    let bytes = read_file(path)?;
    let layout: Layout = serde_json::from_slice(&bytes).map_err(|_| {
        Error::Refused(format!(
            "Unrecognized request-storage layout at {}; preserve it for reconciliation",
            path.display()
        ))
    })?;
    if layout.layout_version != 1 || layout.state_path != STATE_PATH {
        return Err(Error::Refused(format!(
            "Unsupported request-storage layout at {}; use a compatible Orbit Research binary",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn no_symlinks(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                current.pop();
            }
            _ => current.push(component.as_os_str()),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Refused(format!(
                    "Request storage cannot follow symlinks: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn require_directory(path: &Path) -> Result<()> {
    no_symlinks(path)?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::Refused(format!(
            "Request storage is not a directory at {}",
            path.display()
        )));
    }
    Ok(())
}

fn create_directory(path: &Path) -> Result<()> {
    no_symlinks(path)?;
    if path.exists() {
        return require_directory(path);
    }
    fs::create_dir(path)?;
    sync_directory(
        path.parent()
            .ok_or_else(|| Error::Internal("Missing directory parent".into()))?,
    )
}

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = open_read_file(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn open_read_file(path: &Path) -> Result<File> {
    no_symlinks(path)?;
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Refused(format!(
            "Request-log entry is not an ordinary file at {}",
            path.display()
        )));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Refused(format!(
            "Request-log entry is not an ordinary file at {}",
            path.display()
        )));
    }
    Ok(file)
}

fn locked(path: &Path) -> Result<File> {
    no_symlinks(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(Error::Refused(
                "Request-storage lock is not an ordinary file".into(),
            ));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let lock = options.open(path)?;
    if !lock.metadata()?.is_file() {
        return Err(Error::Refused(
            "Request-storage lock is not an ordinary file".into(),
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(Error::Refused("Request storage is busy; retry preparation after active link operations finish".into()));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

struct OwnedMarker {
    path: PathBuf,
    file: File,
    armed: bool,
}

impl Drop for OwnedMarker {
    fn drop(&mut self) {
        if self.armed && same_file(&self.file, &self.path).unwrap_or(false) {
            let _ = fs::remove_file(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = sync_directory(parent);
            }
        }
    }
}

fn marker(path: &Path) -> Result<OwnedMarker> {
    if path.exists() {
        check_marker(path)?;
        return Ok(OwnedMarker {
            path: path.into(),
            file: open_read_file(path)?,
            armed: false,
        });
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Internal("Missing marker parent".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(MARKER)?;
    temp.as_file().sync_all()?;
    let file = temp
        .persist_noclobber(path)
        .map_err(|error| Error::Io(error.error))?;
    sync_directory(parent)?;
    Ok(OwnedMarker {
        path: path.into(),
        file,
        armed: true,
    })
}

pub(crate) fn same_file(file: &File, path: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        _ => return Ok(false),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let original = file.metadata()?;
        Ok(original.dev() == metadata.dev() && original.ino() == metadata.ino())
    }
    #[cfg(not(unix))]
    Ok(file.metadata()?.len() == metadata.len())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn exchange(legacy: &Path, state: &Path) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let source = CString::new(legacy.as_os_str().as_bytes())
            .map_err(|_| Error::Invalid("NUL in request-storage path".into()))?;
        let target = CString::new(state.as_os_str().as_bytes())
            .map_err(|_| Error::Invalid("NUL in request-storage path".into()))?;
        // Both names are verified owned entries on the same filesystem. The
        // OS swaps them atomically, retaining open descriptors and lock inodes.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        };
        #[cfg(target_os = "macos")]
        let result =
            unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_SWAP) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        Err(Error::Refused(format!(
            "Cannot atomically prepare request storage from {} to {}: {error}; the original log is preserved",
            legacy.display(),
            state.display()
        )))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(Error::Refused(format!(
        "Atomic request-storage preparation is unavailable from {} to {}",
        legacy.display(),
        state.display()
    )))
}
