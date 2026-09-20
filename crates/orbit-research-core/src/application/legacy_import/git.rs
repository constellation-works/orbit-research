//! Read-only Git subprocess helper, duplicated from the pattern in
//! `src/orbit_research/importers.py::git`. This crate must never depend on
//! `orbit-research-owner`, so `Owner::apply` stays unreachable from an
//! adapter; a thin, write-free helper is duplicated here instead. If this
//! grows, extract a dedicated `orbit-research-git` crate rather than adding
//! write operations here.

use std::path::Path;
use std::process::Command;

/// Run `git -C root <args>`. Returns trimmed stdout on success, `None` on
/// any failure (non-zero exit, missing binary, not a Git checkout).
pub(crate) fn run(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
