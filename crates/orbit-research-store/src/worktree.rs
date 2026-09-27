//! Worktree mode: an agent's validated, uncommitted write of its reserved R.
//! The run's own commit step publishes the files; this writer never allocates
//! IDs, commits, or touches any other record.
use crate::{
    Error, Result,
    corpus::Corpus,
    edit::{self, Edit},
    record,
    writer::{atomic_write, exclusive_lock, refuse_unsafe_path, safe_create_dirs},
};
use serde::Serialize;
use std::{fs, path::Path};

/// Files written in a run worktree, uncommitted.
#[derive(Debug, Serialize)]
pub struct WorktreeWrite {
    pub id: String,
    pub path: String,
    /// Git blob of the written record; the expected blob for the next revise.
    pub blob: String,
    pub files: Vec<String>,
}

/// Names the one reserved R this worktree writes, set by its first write.
pub(crate) const BINDING: &str = "orbit-research-reserved";

impl Corpus {
    pub(crate) fn write_reserved(
        &self,
        id: &str,
        expected_blob: &str,
        edit: &Edit,
    ) -> Result<WorktreeWrite> {
        let (own, _) = self.git_dirs()?;
        let _lock = exclusive_lock(&own.join("orbit-research-writer.lock"))?;
        self.require_open_schema()?;
        let head = self.committed_snapshot()?;
        let reserved = head
            .records
            .iter()
            .find(|r| r.id == id && r.kind == "R")
            .filter(|r| matches!(r.metadata["status"].as_str(), Some("planned" | "running")))
            .ok_or_else(|| {
                Error::Refused(format!(
                    "Worktree mode writes only this run's reserved research record; {id} is not a reserved R stub at the worktree's HEAD"
                ))
            })?;
        let binding = own.join(BINDING);
        let bound = match fs::read_to_string(&binding) {
            Ok(bound) => Some(bound.trim().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(bound) = &bound
            && bound != id
        {
            return Err(Error::Refused(format!(
                "This worktree writes only its reserved research record {bound}"
            )));
        }

        let snapshot = self.snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == id && r.path == reserved.path)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{id} is missing from the worktree at {}",
                    reserved.path
                ))
            })?;
        let text = edit::revise(&self.contract, record, edit, &record::utc_date()?)?;
        edit::check_records(&self.contract, &snapshot.records, &record.path, &text)?;
        let mut files = vec![(record.path.clone(), text)];
        if let Some(manifest) = &edit.manifest {
            let directory = Path::new(&record.path)
                .parent()
                .and_then(Path::to_str)
                .ok_or_else(|| Error::Invalid("Invalid research path".into()))?;
            files.push((
                format!("{directory}/data/manifest.json"),
                edit::manifest_text(&self.contract, manifest)?,
            ));
        }
        for (path, _) in &files {
            refuse_unsafe_path(self.root(), path)?;
        }
        if record.git_blob != expected_blob {
            // An identical retry finds its own bytes already in place and adopts them.
            let applied = files.iter().all(|(path, text)| {
                fs::read(self.root().join(path)).ok().as_deref() == Some(text.as_bytes())
            });
            if !applied {
                return Err(Error::Conflict(format!(
                    "{id} changed since it was opened; reload before editing"
                )));
            }
        } else {
            for (path, text) in &files {
                let path = self.root().join(path);
                let parent = path
                    .parent()
                    .ok_or_else(|| Error::Invalid("Invalid write path".into()))?;
                safe_create_dirs(self.root(), parent)?;
                atomic_write(&path, text.as_bytes(), true)?;
            }
        }
        if bound.is_none() {
            atomic_write(&binding, format!("{id}\n").as_bytes(), false)?;
        }
        Ok(WorktreeWrite {
            id: id.into(),
            path: record.path.clone(),
            blob: self.hash_bytes(files[0].1.as_bytes())?,
            files: files.into_iter().map(|(path, _)| path).collect(),
        })
    }
}
