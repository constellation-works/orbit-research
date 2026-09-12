//! Git as an explicit subprocess, hidden behind a trait.
//!
//! The scientific pin is exactly what `git show` / `rev-parse` / `ls-tree` returned for an
//! explicit revision, including null pins and dirty-tree detection. No `libgit2`, no
//! implicit repository discovery, and never a HEAD fallback for a requested revision.

use std::path::Path;
use std::process::Command;

/// Read-only Git access for one explicit checkout root.
pub trait Git: Send + Sync {
    /// Trimmed stdout, or `None` when Git exited non-zero (the Python `git()` contract).
    fn text(&self, root: &Path, args: &[&str]) -> Option<String>;

    /// Raw stdout bytes, or `None` when Git exited non-zero.
    fn bytes(&self, root: &Path, args: &[&str]) -> Option<Vec<u8>>;
}

/// The supported implementation: `git` on PATH with optional locks disabled.
#[derive(Clone, Copy, Debug, Default)]
pub struct SubprocessGit;

impl SubprocessGit {
    fn output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root.as_os_str())
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    }
}

impl Git for SubprocessGit {
    fn text(&self, root: &Path, args: &[&str]) -> Option<String> {
        let stdout = Self::output(root, args)?;
        Some(String::from_utf8(stdout).ok()?.trim().to_owned())
    }

    fn bytes(&self, root: &Path, args: &[&str]) -> Option<Vec<u8>> {
        Self::output(root, args)
    }
}

/// A full 40- or 64-hex Git revision. Abbreviations and symbolic names are refused so a
/// snapshot request can never silently resolve to whatever HEAD happens to be.
pub fn full_revision(revision: &str) -> bool {
    (revision.len() == 40 || revision.len() == 64)
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
