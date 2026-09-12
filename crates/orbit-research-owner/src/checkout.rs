//! An explicit scientific checkout: its Git toplevel, its committed bytes, its safe paths.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::git::{Git, full_revision};
use crate::{Result, require};

/// A resolved Git checkout root that the caller named explicitly.
#[derive(Clone)]
pub struct Checkout {
    root: PathBuf,
    git: Arc<dyn Git>,
    /// Content at an exact full revision is immutable, so reads of it are memoised.
    /// Nothing reachable only through HEAD or the working tree is ever cached.
    snapshots: Arc<Mutex<Snapshots>>,
}

#[derive(Default)]
struct Snapshots {
    blobs: HashMap<(String, String), Vec<u8>>,
    listings: HashMap<String, String>,
}

impl Checkout {
    /// Open an explicit checkout root. The path must be the Git toplevel itself and must
    /// already carry at least one commit; a subdirectory or an empty repository is refused.
    pub fn open(path: &Path, git: Arc<dyn Git>) -> Result<Self> {
        let root = path
            .canonicalize()
            .map_err(|error| crate::OwnerError::Invalid(format!("{}: {error}", path.display())))?;
        let toplevel = git.text(&root, &["rev-parse", "--show-toplevel"]);
        require(
            toplevel.as_deref().map(Path::new).map(Path::to_path_buf) == Some(root.clone()),
            "owner/source root must be an explicit Git checkout root",
        )?;
        require(
            git.text(&root, &["rev-parse", "HEAD"]).is_some(),
            "owner requires an initial Git commit",
        )?;
        Ok(Self {
            root,
            git,
            snapshots: Arc::new(Mutex::new(Snapshots::default())),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Trimmed Git stdout, or `None` when the command failed.
    pub fn text(&self, args: &[&str]) -> Option<String> {
        self.git.text(&self.root, args)
    }

    /// The inspected HEAD of this checkout. Used for author-time provenance only.
    pub fn head(&self) -> Result<String> {
        self.text(&["rev-parse", "HEAD"]).ok_or_else(|| {
            crate::OwnerError::Invalid("owner requires an initial Git commit".into())
        })
    }

    /// Committed bytes at an exact full revision. Never falls back to HEAD or the worktree.
    pub fn git_bytes(&self, revision: &str, path: &str) -> Result<Vec<u8>> {
        require(full_revision(revision), "full Git revision required")?;
        let key = (revision.to_owned(), path.to_owned());
        if let Ok(snapshots) = self.snapshots.lock()
            && let Some(bytes) = snapshots.blobs.get(&key)
        {
            return Ok(bytes.clone());
        }
        let bytes = self
            .git
            .bytes(&self.root, &["show", &format!("{revision}:{path}")])
            .ok_or_else(|| {
                crate::OwnerError::Invalid(format!("no committed source at {revision}:{path}"))
            })?;
        if let Ok(mut snapshots) = self.snapshots.lock() {
            snapshots.blobs.insert(key, bytes.clone());
        }
        Ok(bytes)
    }

    /// Every path in an exact committed tree, or `None` when the revision is unavailable.
    pub fn listing(&self, revision: &str) -> Option<String> {
        if !full_revision(revision) {
            return self.text(&["ls-tree", "-r", "--name-only", revision]);
        }
        if let Ok(snapshots) = self.snapshots.lock()
            && let Some(listing) = snapshots.listings.get(revision)
        {
            return Some(listing.clone());
        }
        let listing = self.text(&["ls-tree", "-r", "--name-only", revision])?;
        if let Ok(mut snapshots) = self.snapshots.lock() {
            snapshots
                .listings
                .insert(revision.to_owned(), listing.clone());
        }
        Some(listing)
    }

    /// Resolve a relative canonical path, refusing escapes and every symlinked component.
    pub fn safe_path(&self, relative: &str) -> Result<PathBuf> {
        safe_path(&self.root, relative)
    }
}

/// Path containment for canonical records: relative, no `..`, no `.git`/`.orbit`, no symlink.
pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    let mut parts = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => parts.push(part),
            Component::CurDir => {}
            _ => {
                return Err(crate::OwnerError::Invalid(
                    "path must remain inside the scientific owner root".into(),
                ));
            }
        }
    }
    require(
        !candidate.is_absolute()
            && !parts.is_empty()
            && !parts
                .iter()
                .any(|part| matches!(part.to_str(), Some(".git" | ".orbit" | ".."))),
        "path must remain inside the scientific owner root",
    )?;
    let mut current = root.to_path_buf();
    for part in &parts {
        current.push(part);
        require(
            !current
                .symlink_metadata()
                .is_ok_and(|data| data.file_type().is_symlink()),
            "symlinks are not allowed in canonical paths",
        )?;
    }
    if let Ok(resolved) = current.canonicalize() {
        require(resolved.starts_with(root), "path escapes owner root")?;
    }
    Ok(current)
}
