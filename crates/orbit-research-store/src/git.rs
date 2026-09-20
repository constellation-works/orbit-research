//! Git process boundary. Bytes never pass through trimmed text helpers.
use crate::{Error, Result, corpus::Corpus};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn command_error(operation: &str, output: &Output) -> Error {
    Error::Invalid(format!(
        "git {operation} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

impl Corpus {
    pub fn published(&self, revision: &str, reference: &str) -> Result<bool> {
        let output = Command::new("git")
            .arg("--literal-pathspecs")
            .arg("-C")
            .arg(self.root())
            .args(["merge-base", "--is-ancestor", revision, reference])
            .output()?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(command_error("merge-base", &output)),
        }
    }

    pub fn committed_blob(&self, revision: &str, path: &str) -> Result<String> {
        self.git(&["rev-parse", &format!("{revision}:{path}")])
    }

    pub fn committed_bytes(&self, revision: &str, path: &str) -> Result<Vec<u8>> {
        let mode = self.git(&["ls-tree", revision, "--", path])?;
        if !mode.starts_with("100644 blob ") && !mode.starts_with("100755 blob ") {
            return Err(Error::Invalid(
                "Evidence must reference regular committed files".into(),
            ));
        }
        let out = Command::new("git")
            .arg("--literal-pathspecs")
            .arg("-C")
            .arg(self.root())
            .args(["show", &format!("{revision}:{path}")])
            .output()?;
        if !out.status.success() {
            return Err(command_error("show", &out));
        }
        Ok(out.stdout)
    }

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
            return Err(command_error(
                args.first().copied().unwrap_or("command"),
                &result,
            ));
        }
        Ok(result.stdout)
    }

    pub(crate) fn hash_bytes(&self, bytes: &[u8]) -> Result<String> {
        let mut child = Command::new("git")
            .arg("--literal-pathspecs")
            .arg("-C")
            .arg(self.root())
            .args(["hash-object", "--stdin", "--no-filters"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| Error::Invalid("Missing Git stdin".into()))?
            .write_all(bytes);
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(command_error("hash-object", &output));
        }
        write_result?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().into())
    }
    pub(crate) fn committed_paths(&self, revision: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .arg("--literal-pathspecs")
            .arg("-C")
            .arg(self.root())
            .args(["ls-tree", "-r", "--name-only", "-z", revision])
            .output()?;
        if !output.status.success() {
            return Err(command_error("ls-tree", &output));
        }
        output
            .stdout
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                String::from_utf8(b.to_vec())
                    .map_err(|_| Error::Invalid("Non UTF-8 committed path".into()))
            })
            .collect()
    }
}
