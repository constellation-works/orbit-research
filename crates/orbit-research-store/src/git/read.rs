//! In-process Git reads for the sandboxed plugin's read and validate paths.
//!
//! Nothing here may spawn a process: refs, commits, trees and blobs are read
//! directly from the on-disk object database (loose or packed, including
//! packed-refs and detached HEAD) via `gix`. `git/mod.rs` retains the process
//! boundary for the primary-mode writer's commit/lock plumbing.
use crate::{Error, Result};
use gix::bstr::ByteSlice;
use sha1::{Digest, Sha1};
use std::path::Path;

fn open(root: &Path) -> Result<gix::Repository> {
    gix::open(root).map_err(|_| {
        Error::Invalid(format!(
            "Corpus at {} is not a Git repository; initialize or select a Git repository for the corpus",
            root.display()
        ))
    })
}

pub(crate) fn ensure_repository(root: &Path) -> Result<()> {
    open(root).map(|_| ())
}

/// This checkout's own Git directory: `<common>/worktrees/<name>` in a linked
/// worktree, the repository's `.git` in the primary checkout.
pub(crate) fn own_git_dir(root: &Path) -> Result<std::path::PathBuf> {
    Ok(open(root)?.git_dir().to_owned())
}

/// Whether `HEAD` currently resolves to a commit (false for a freshly
/// initialized repository with no commits yet).
pub(crate) fn has_head(root: &Path) -> Result<bool> {
    let repo = open(root)?;
    Ok(!repo
        .head()
        .map_err(|error| Error::Internal(error.to_string()))?
        .is_unborn())
}

fn no_commits_error(root: &Path) -> Error {
    Error::Invalid(format!(
        "Corpus has no commits; inspect and preserve its files, then either commit them explicitly or move them aside before rerunning workspace init at {}",
        root.display()
    ))
}

/// The commit `HEAD` currently resolves to, as a hex object id. Handles loose
/// and packed refs and a detached `HEAD` transparently; an unborn `HEAD`
/// (no commits yet) yields a typed error rather than a raw Git message.
pub(crate) fn head_commit(root: &Path) -> Result<String> {
    let repo = open(root)?;
    let mut head = repo
        .head()
        .map_err(|error| Error::Internal(error.to_string()))?;
    if head.is_unborn() {
        return Err(no_commits_error(root));
    }
    let commit = head
        .peel_to_commit()
        .map_err(|error| Error::Internal(error.to_string()))?;
    Ok(commit.id.to_string())
}

/// Resolve any revision spec (a full object id, `HEAD`, a branch or tag name,
/// or similar) to the commit it identifies, peeling through tags.
fn resolve_commit<'repo>(
    repo: &'repo gix::Repository,
    revision: &str,
) -> Result<gix::Commit<'repo>> {
    let id = repo
        .rev_parse_single(revision)
        .map_err(|_| Error::Invalid(format!("Unknown revision: {revision}")))?;
    id.object()
        .map_err(|error| Error::Internal(error.to_string()))?
        .peel_to_commit()
        .map_err(|_| Error::Invalid(format!("Revision {revision} is not a commit")))
}

fn find_tree_entry<'repo>(
    commit: &gix::Commit<'repo>,
    path: &str,
) -> Result<Option<gix::object::tree::Entry<'repo>>> {
    let tree = commit
        .tree()
        .map_err(|error| Error::Internal(error.to_string()))?;
    tree.lookup_entry(path.split('/'))
        .map_err(|error| Error::Internal(error.to_string()))
}

/// Equivalent to `git rev-parse revision:path`: the object id at `path`
/// within `revision`, regardless of the entry's type.
pub(crate) fn committed_blob(root: &Path, revision: &str, path: &str) -> Result<String> {
    let repo = open(root)?;
    let commit = resolve_commit(&repo, revision)?;
    let entry = find_tree_entry(&commit, path)?
        .ok_or_else(|| Error::Invalid(format!("No such committed path: {path}")))?;
    Ok(entry.object_id().to_string())
}

/// Equivalent to `git show revision:path`, refusing anything but a regular
/// (non-executable or executable) committed file.
pub(crate) fn committed_bytes(root: &Path, revision: &str, path: &str) -> Result<Vec<u8>> {
    let repo = open(root)?;
    let commit = resolve_commit(&repo, revision)?;
    let entry = find_tree_entry(&commit, path)?
        .ok_or_else(|| Error::Invalid(format!("No such committed path: {path}")))?;
    if !entry.mode().is_blob() {
        return Err(Error::Invalid(
            "Evidence must reference regular committed files".into(),
        ));
    }
    Ok(entry
        .object()
        .map_err(|error| Error::Internal(error.to_string()))?
        .data
        .clone())
}

/// Equivalent to `git ls-tree -r --name-only revision`: every blob and
/// submodule path reachable from `revision`'s tree, recursively.
pub(crate) fn committed_paths(root: &Path, revision: &str) -> Result<Vec<String>> {
    let repo = open(root)?;
    let commit = resolve_commit(&repo, revision)?;
    let tree = commit
        .tree()
        .map_err(|error| Error::Internal(error.to_string()))?;
    let mut paths = Vec::new();
    collect_paths(&repo, tree, "", &mut paths)?;
    Ok(paths)
}

fn collect_paths(
    repo: &gix::Repository,
    tree: gix::Tree<'_>,
    prefix: &str,
    out: &mut Vec<String>,
) -> Result<()> {
    let decoded = tree
        .decode()
        .map_err(|error| Error::Internal(error.to_string()))?;
    for entry in &decoded.entries {
        let name = entry
            .filename
            .to_str()
            .map_err(|_| Error::Invalid("Non UTF-8 committed path".into()))?;
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode.is_tree() {
            let subtree = repo
                .find_tree(entry.oid.to_owned())
                .map_err(|error| Error::Internal(error.to_string()))?;
            collect_paths(repo, subtree, &path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Equivalent to `git merge-base --is-ancestor revision reference`: whether
/// `revision` is `reference` or reachable by walking its ancestry.
pub(crate) fn published(root: &Path, revision: &str, reference: &str) -> Result<bool> {
    let repo = open(root)?;
    let revision_commit = resolve_commit(&repo, revision)?;
    let reference_commit = resolve_commit(&repo, reference)?;
    if revision_commit.id == reference_commit.id {
        return Ok(true);
    }
    Ok(repo
        .merge_base(revision_commit.id, reference_commit.id)
        .map(|base| base == revision_commit.id)
        .unwrap_or(false))
}

/// The Git blob object id for `bytes`, equivalent to
/// `git hash-object --stdin --no-filters`. Pure and repository-independent.
pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(b"blob ");
    hasher.update(bytes.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
