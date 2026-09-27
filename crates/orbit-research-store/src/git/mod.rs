//! Git process boundary. Bytes never pass through trimmed text helpers.
//!
//! Only the primary-mode writer's plumbing (`git`/`git_bytes`, used by
//! `writer.rs` for commit/lock work) spawns a process here. Every read used by
//! the plugin-sandboxed read/validate path is in-process; see [`read`].
use crate::{Error, Result, corpus::Corpus};
use std::process::{Command, Output};

pub(crate) mod read;

fn command_error(operation: &str, output: &Output) -> Error {
    Error::Internal(format!(
        "git {operation} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

impl Corpus {
    pub(crate) fn ensure_repository(&self) -> Result<()> {
        read::ensure_repository(self.root())
    }

    /// The commit HEAD currently resolves to, as a hex object id.
    pub(crate) fn head_commit(&self) -> Result<String> {
        read::head_commit(self.root())
    }

    pub fn published(&self, revision: &str, reference: &str) -> Result<bool> {
        read::published(self.root(), revision, reference)
    }

    pub fn committed_blob(&self, revision: &str, path: &str) -> Result<String> {
        read::committed_blob(self.root(), revision, path)
    }

    pub fn committed_bytes(&self, revision: &str, path: &str) -> Result<Vec<u8>> {
        read::committed_bytes(self.root(), revision, path)
    }

    /// Escape hatch for the primary-mode writer's commit/lock plumbing. Never
    /// call this from a read or validation path; use [`read`] instead.
    pub(crate) fn git(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.git_bytes(args)?)
            .trim()
            .into())
    }

    pub(crate) fn git_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let result = Command::new("git")
            .arg("--literal-pathspecs")
            .arg("-C")
            .arg(self.root())
            .args(args)
            .output()?;
        if !result.status.success() {
            if args == ["rev-parse", "HEAD"] || args == ["rev-parse", "--verify", "HEAD^{commit}"] {
                let stderr = String::from_utf8_lossy(&result.stderr);
                if stderr.contains("ambiguous argument 'HEAD'")
                    || stderr.contains("Needed a single revision")
                {
                    return Err(Error::Invalid(format!(
                        "Corpus has no commits; inspect and preserve its files, then either commit them explicitly or move them aside before rerunning workspace init at {}",
                        self.root().display()
                    )));
                }
            }
            return Err(command_error(
                args.first().copied().unwrap_or("command"),
                &result,
            ));
        }
        Ok(result.stdout)
    }

    pub(crate) fn hash_bytes(&self, bytes: &[u8]) -> Result<String> {
        Ok(read::hash_bytes(bytes))
    }

    pub(crate) fn committed_paths(&self, revision: &str) -> Result<Vec<String>> {
        read::committed_paths(self.root(), revision)
    }
}
