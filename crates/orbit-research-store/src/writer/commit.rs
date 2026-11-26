//! Stage owned files, reconcile uncertain commits and unwind refused writes.

use super::{ReservationIntent, WriteFile, Writer, atomic_write};
use crate::{Error, Result, git};
use std::{fs, io, path::Path};

impl Writer<'_> {
    pub(super) fn commit(&self, key: &str, intent: &ReservationIntent) -> Result<String> {
        if self.corpus.git(&["rev-parse", "HEAD"])? != intent.parent {
            return Err(Error::Conflict(
                "Corpus advanced before commit; reconcile incomplete write".into(),
            ));
        }
        self.has_staged_files(intent)?;
        let mut add = vec!["add", "--"];
        add.extend(intent.files.iter().map(|file| file.path.as_str()));
        self.run_write_command(key, intent, &add)?;
        if self.has_staged_files(intent)? {
            self.run_write_command(
                key,
                intent,
                &[
                    "commit",
                    "-m",
                    &format!(
                        "Write research record {}\n\nOrbit-Research-Request: {key}",
                        intent.id
                    ),
                ],
            )?;
        }
        self.corpus.git(&["rev-parse", "HEAD"])
    }

    fn run_write_command(
        &self,
        key: &str,
        intent: &ReservationIntent,
        args: &[&str],
    ) -> Result<()> {
        let operation = args[0];
        let output = git::command(self.corpus.root())
            .arg("--literal-pathspecs")
            .args(args)
            .output()
            .map_err(|error| {
                Error::Internal(format!(
                    "Could not confirm git {operation}: {error}. Fix the cause and repeat the same command to recover the write"
                ))
            })?;
        if output.status.success() {
            return Ok(());
        }
        let error = git::command_error(operation, &output);
        if output.status.code().is_none()
            || self.corpus.git(&["rev-parse", "HEAD"]).ok().as_deref()
                != Some(intent.parent.as_str())
        {
            return Err(Error::Internal(format!(
                "{error}\nThe write outcome is uncertain. Repeat the same command to recover it"
            )));
        }
        match self
            .rollback(intent)
            .and_then(|()| self.discard_intent(key))
        {
            Ok(()) => Err(Error::Internal(format!(
                "{error}\nThe write was rolled back. Fix the cause above, then repeat the same command to try again"
            ))),
            Err(rollback) => Err(Error::Internal(format!(
                "{error}\nCould not finish rolling back the write: {rollback}. Reconcile the affected files and index before retrying"
            ))),
        }
    }

    fn rollback(&self, intent: &ReservationIntent) -> Result<()> {
        for file in &intent.files {
            self.check_file(file)?;
        }
        let mut diff = vec!["diff", "--cached", "--raw", "--no-renames", "-z", "--"];
        diff.extend(intent.files.iter().map(|file| file.path.as_str()));
        let staged = self.corpus.git_bytes(&diff)?;
        let mut paths = Vec::new();
        let mut fields = staged.split(|&byte| byte == 0).filter(|s| !s.is_empty());
        while let Some(header) = fields.next() {
            let path = fields
                .next()
                .ok_or_else(|| Error::Internal("Could not read staged changes".into()))?;
            let file = self.check_staged_file(intent, path)?;
            // The writer preserves existing file modes and creates regular files.
            let mut modes = header.split(|&byte| byte == b' ');
            let expected = match modes.next() {
                Some(b":000000") => Some(b"100644".as_slice()),
                mode => mode.and_then(|mode| mode.strip_prefix(b":")),
            };
            if modes.next() != expected {
                return Err(Error::Conflict(format!(
                    "Staged path {} has a conflicting file mode",
                    file.path
                )));
            }
            paths.push(file.path.as_str());
        }
        if !paths.is_empty() {
            let mut reset = vec!["reset", "-q", intent.parent.as_str(), "--"];
            reset.extend(paths);
            self.corpus.git(&reset)?;
        }
        for file in &intent.files {
            self.check_file(file)?;
            let path = self.corpus.root().join(&file.path);
            match &file.before {
                Some(before) => atomic_write(&path, before.as_bytes(), true)?,
                None => ignore_missing(fs::remove_file(path))?,
            }
        }
        if intent.id.starts_with('R')
            && let Some(parent) = Path::new(&intent.path).parent()
        {
            let record_dir = self.corpus.root().join(parent);
            for dir in ["code", "artifacts", "data"] {
                ignore_missing(fs::remove_dir(record_dir.join(dir)))?;
            }
            ignore_missing(fs::remove_dir(&record_dir))?;
        }
        Ok(())
    }

    fn has_staged_files(&self, intent: &ReservationIntent) -> Result<bool> {
        let staged =
            self.corpus
                .git_bytes(&["diff", "--cached", "--no-renames", "--name-only", "-z"])?;
        for path in staged
            .split(|&byte| byte == 0)
            .filter(|path| !path.is_empty())
        {
            self.check_staged_file(intent, path)?;
        }
        Ok(!staged.is_empty())
    }

    fn check_staged_file<'a>(
        &self,
        intent: &'a ReservationIntent,
        path: &[u8],
    ) -> Result<&'a WriteFile> {
        let file = intent
            .files
            .iter()
            .find(|f| f.path.as_bytes() == path)
            .ok_or_else(|| {
                Error::Invalid("Unrelated staged changes appeared; refusing commit".into())
            })?;
        let bytes = self
            .corpus
            .git_bytes(&["show", &format!(":{}", file.path)])?;
        if bytes != file.text.as_bytes()
            && file.before.as_ref().is_none_or(|s| bytes != s.as_bytes())
        {
            return Err(Error::Conflict(format!(
                "Staged path {} has conflicting edits; refusing commit",
                file.path
            )));
        }
        Ok(file)
    }

    pub(super) fn recover_commit(
        &self,
        key: &str,
        intent: &ReservationIntent,
        head: &str,
    ) -> Result<String> {
        let commit = self
            .find_request_commit(key, intent, head)?
            .ok_or_else(|| {
                Error::Invalid(
                    "Corpus advanced during incomplete reservation; manual reconciliation required"
                        .into(),
                )
            })?;
        let changed = self.corpus.git_bytes(&[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &intent.parent,
            &commit,
            "--",
        ])?;
        for path in changed
            .split(|&byte| byte == 0)
            .filter(|path| !path.is_empty())
        {
            if !intent.files.iter().any(|file| file.path.as_bytes() == path) {
                return Err(Error::Invalid(
                    "Recovered commit contains unrelated paths".into(),
                ));
            }
        }
        Ok(commit)
    }

    /// Locate the request by its trailer and first parent, even below HEAD.
    pub(super) fn find_request_commit(
        &self,
        key: &str,
        intent: &ReservationIntent,
        head: &str,
    ) -> Result<Option<String>> {
        let matches = self.corpus.git(&[
            "-c",
            "trailer.separators=:",
            "log",
            "-z",
            "--no-show-signature",
            "--format=%H%x00%P%x00%(trailers:key=Orbit-Research-Request,valueonly)",
            "--fixed-strings",
            &format!("--grep=Orbit-Research-Request: {key}"),
            &format!("{}..{head}", intent.parent),
            "--",
        ])?;
        let mut fields = matches.split_terminator('\0');
        while let Some(commit) = fields.next() {
            let (Some(parents), Some(trailers)) = (fields.next(), fields.next()) else {
                return Err(Error::Internal(
                    "Could not read possible request commits".into(),
                ));
            };
            if parents.split_whitespace().next() == Some(intent.parent.as_str())
                && trailers.lines().any(|line| line == key)
            {
                return Ok(Some(commit.to_owned()));
            }
        }
        Ok(None)
    }
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
