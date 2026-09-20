//! Serialized, schema-aware writes on the explicitly selected integration checkout.
//! A committed stub is allocated before any worker is dispatched.
use crate::{Error, Result, corpus::Corpus};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

pub use orbit_research_common::Reservation;
#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    request_digest: String,
    id: String,
    path: String,
    text: String,
    parent: String,
    reservation: Option<Reservation>,
}
impl Corpus {
    /// Revise a question only when the caller still holds its exact content identity.
    pub fn revise_question(
        &self,
        id: &str,
        expected_blob: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
    ) -> Result<Reservation> {
        let common = PathBuf::from(self.git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?);
        let state = common.join("orbit-research-writer");
        fs::create_dir_all(&state)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(state.join("lock"))?;
        lock.lock_exclusive()?;
        if !self.git(&["status", "--porcelain"])?.is_empty() {
            return Err(Error::Invalid(
                "Revision requires a clean corpus checkout".into(),
            ));
        }
        let snapshot = self.snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == id && r.kind == "Q")
            .ok_or_else(|| Error::Invalid("Only existing questions are editable".into()))?;
        if record.git_blob != expected_blob {
            return Err(Error::Invalid(
                "Question changed since it was opened; reload before editing".into(),
            ));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("Title is required".into()));
        }
        let mut metadata = record.metadata.clone();
        // Preserve the frozen filename when a question's wording changes.
        let filename = std::path::Path::new(&record.path)
            .file_stem()
            .and_then(|p| p.to_str())
            .ok_or_else(|| Error::Invalid("Invalid question filename".into()))?;
        metadata["slug"] = json!(
            filename
                .split_once('-')
                .ok_or_else(|| Error::Invalid("Missing frozen slug".into()))?
                .1
        );
        metadata["title"] = json!(title);
        metadata["tags"] = json!(tags);
        let date = Command::new("date").args(["-u", "+%Y-%m-%d"]).output()?;
        if !date.status.success() {
            return Err(Error::Invalid("Cannot read UTC date".into()));
        }
        metadata["updated"] = json!(String::from_utf8_lossy(&date.stdout).trim());
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&self.schema)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if !validator.is_valid(&metadata) {
            return Err(Error::Invalid(
                "Question revision violates owner schema".into(),
            ));
        }
        let text = format!("---\n{}---\n\n{body}\n", serde_yaml::to_string(&metadata)?);
        let path = self.root().join(&record.path);
        let parent = path
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid record path".into()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(text.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).map_err(|e| Error::Io(e.error))?;
        self.git(&["add", "--", &record.path])?;
        if !self
            .git(&["diff", "--cached", "--name-only"])?
            .lines()
            .all(|p| p == record.path)
        {
            return Err(Error::Invalid(
                "Unrelated staged changes appeared; refusing commit".into(),
            ));
        }
        if !self.git(&["diff", "--cached", "--name-only"])?.is_empty() {
            self.git(&["commit", "-m", &format!("Revise research question {id}")])?;
        }
        Ok(Reservation {
            id: id.into(),
            path: record.path.clone(),
            commit: self.git(&["rev-parse", "HEAD"])?,
            request_digest: digest(text.as_bytes()),
        })
    }
    /// `request_key` is caller-generated once and retained across uncertain responses.
    /// Reusing it with a different request is refused. The owner checkout must be clean.
    pub fn reserve(
        &self,
        request_key: &str,
        kind: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
        derived_from: Vec<String>,
    ) -> Result<Reservation> {
        if request_key.is_empty() || request_key.len() > 256 {
            return Err(Error::Invalid(
                "Request key must contain 1–256 bytes".into(),
            ));
        }
        if !matches!(kind, "Q" | "H" | "T" | "R") {
            return Err(Error::Invalid("Record kind must be Q, H, T or R".into()));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("Title is required".into()));
        }
        let common = PathBuf::from(self.git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?);
        let own_git = PathBuf::from(self.git(&["rev-parse", "--absolute-git-dir"])?);
        if own_git.canonicalize()? != common.canonicalize()? {
            return Err(Error::Invalid(
                "Reserve IDs on the primary integration checkout before dispatching a worktree"
                    .into(),
            ));
        }
        let state = common.join("orbit-research-writer");
        fs::create_dir_all(&state)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(state.join("lock"))?;
        lock.lock_exclusive()?;
        let key = digest(request_key.as_bytes());
        let request_digest = digest(&serde_json::to_vec(&json!([
            kind,
            title,
            body,
            tags,
            derived_from
        ]))?);
        let journal_path = state.join(format!("{key}.json"));
        let mut journal: Journal = if journal_path.exists() {
            let old: Journal = serde_json::from_slice(&fs::read(&journal_path)?)?;
            if old.request_digest != request_digest {
                return Err(Error::Invalid(
                    "Request key was already used for different content".into(),
                ));
            }
            if let Some(reservation) = &old.reservation {
                self.git(&[
                    "cat-file",
                    "-e",
                    &format!("{}:{}", reservation.commit, reservation.path),
                ])?;
                return Ok(reservation.clone());
            }
            old
        } else {
            if !self.git(&["status", "--porcelain"])?.is_empty() {
                return Err(Error::Invalid("Capture requires a clean corpus integration checkout; preserve and commit existing edits first".into()));
            }
            let snapshot = self.snapshot()?;
            for parent in &derived_from {
                if !snapshot.records.iter().any(|r| &r.id == parent) {
                    return Err(Error::Invalid(format!("Unknown predecessor {parent}")));
                }
            }
            let next = snapshot
                .records
                .iter()
                .filter(|r| r.kind == kind)
                .filter_map(|r| r.id[1..].parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1;
            if next > 999 {
                return Err(Error::Invalid("Owner ID space exhausted".into()));
            }
            let id = format!("{kind}{next:03}");
            let slug = title
                .to_ascii_lowercase()
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            if slug.is_empty() {
                return Err(Error::Invalid(
                    "Title needs at least one ASCII letter or number for its path".into(),
                ));
            }
            let date = Command::new("date").args(["-u", "+%Y-%m-%d"]).output()?;
            if !date.status.success() {
                return Err(Error::Invalid("Cannot read UTC date".into()));
            }
            let date = String::from_utf8_lossy(&date.stdout).trim().to_owned();
            let mut meta = json!({"id":id,"title":title,"status":match kind {"R"=>"planned","T"=>"active",_=>"open"},"tags":tags,"derived_from":derived_from,"created":date,"updated":date});
            match kind {
                "Q" => {
                    meta["answered_by"] = json!([]);
                }
                "H" => {
                    meta["revision"] = json!(1);
                    meta["assessments"] = json!([]);
                }
                "T" => {
                    meta["claims"] = json!([]);
                    meta["supersedes"] = json!([]);
                }
                _ => {
                    meta["tests"] = json!([]);
                }
            }
            let validator = jsonschema::JSONSchema::options()
                .with_draft(jsonschema::Draft::Draft202012)
                .compile(&self.schema)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if !validator.is_valid(&meta) {
                return Err(Error::Invalid("New record violates owner schema".into()));
            }
            let directory = self.schema["x-observatory"]["kinds"][kind]["directory"]
                .as_str()
                .ok_or_else(|| Error::Invalid("Missing owner directory".into()))?;
            // snapshot() has already validated every existing owner directory for traversal/symlinks.
            let path = if kind != "R" {
                format!("{directory}/{id}-{slug}.md")
            } else {
                format!("{directory}/{id}-{slug}/README.md")
            };
            let body = if kind == "Q" {
                format!("# {id} — {title}\n\n## The question\n\n{body}\n")
            } else if kind == "H" {
                format!(
                    "# {id} — {title}\n\n## The claim\n\n{body}\n\n## What would refute it\n\nTo be specified before testing.\n"
                )
            } else if kind == "T" {
                format!(
                    "# {id} — {title}\n\n## What it says\n\n{body}\n\n## Where it stops\n\nScope and limitations remain to be specified.\n"
                )
            } else {
                format!(
                    "# {id} — {title}\n\n## Question\n\n{body}\n\n## Method\n\nPending.\n\n## Result\n\nPending.\n\n## Limitations\n\nNot yet run.\n\n## Next\n\nAwait dispatch.\n"
                )
            };
            let text = format!("---\n{}---\n\n{body}", serde_yaml::to_string(&meta)?);
            let journal = Journal {
                request_digest,
                id,
                path,
                text,
                parent: snapshot.revision,
                reservation: None,
            };
            write_new(&journal_path, &serde_json::to_vec(&journal)?)?;
            journal
        };
        let head = self.git(&["rev-parse", "HEAD"])?;
        if head != journal.parent {
            // Recover a crash after commit, before the durable return value was written.
            let message = self.git(&["log", "-1", "--format=%B"])?;
            if !message
                .lines()
                .any(|l| l == format!("Orbit-Research-Request: {key}"))
            {
                return Err(Error::Invalid(
                    "Corpus advanced during incomplete reservation; manual reconciliation required"
                        .into(),
                ));
            }
            let committed = self.git(&["show", &format!("HEAD:{}", journal.path)])?;
            if committed.trim_end() != journal.text.trim_end() {
                return Err(Error::Invalid(
                    "Committed reservation content differs".into(),
                ));
            }
        } else {
            let path = self.root().join(&journal.path);
            let parent = path
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid reservation path".into()))?;
            safe_create_dirs(self.root(), parent)?;
            if path.exists() {
                if fs::symlink_metadata(&path)?.file_type().is_symlink()
                    || fs::read_to_string(&path)? != journal.text
                {
                    return Err(Error::Invalid(
                        "Reservation path has conflicting edits".into(),
                    ));
                }
            } else {
                write_new(&path, journal.text.as_bytes())?;
            }
            let mut paths = vec![journal.path.clone()];
            if kind == "R" {
                let manifest = parent.join("data/manifest.json");
                safe_create_dirs(self.root(), &parent.join("data"))?;
                safe_create_dirs(self.root(), &parent.join("code"))?;
                safe_create_dirs(self.root(), &parent.join("artifacts"))?;
                if manifest.exists() {
                    if fs::symlink_metadata(&manifest)?.file_type().is_symlink()
                        || fs::read(&manifest)? != b"{\"inputs\":[]}\n"
                    {
                        return Err(Error::Invalid(
                            "Reserved manifest has conflicting edits".into(),
                        ));
                    }
                } else {
                    write_new(&manifest, b"{\"inputs\":[]}\n")?;
                }
                paths.push(format!(
                    "{}/data/manifest.json",
                    parent
                        .strip_prefix(self.root())
                        .map_err(|e| Error::Invalid(e.to_string()))?
                        .display()
                ));
            }
            self.snapshot()?;
            let mut args = vec!["add", "--"];
            args.extend(paths.iter().map(String::as_str));
            self.git(&args)?;
            let staged = self.git(&["diff", "--cached", "--name-only"])?;
            if staged.lines().any(|p| !paths.iter().any(|ours| ours == p)) {
                return Err(Error::Invalid(
                    "Unrelated staged changes appeared; refusing commit".into(),
                ));
            }
            self.git(&[
                "commit",
                "-m",
                &format!(
                    "Reserve {} for research\n\nOrbit-Research-Request: {key}",
                    journal.id
                ),
            ])?;
        }
        let reservation = Reservation {
            id: journal.id.clone(),
            path: journal.path.clone(),
            commit: self.git(&["rev-parse", "HEAD"])?,
            request_digest: journal.request_digest.clone(),
        };
        journal.reservation = Some(reservation.clone());
        let temporary = journal_path.with_extension("tmp");
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&serde_json::to_vec(&journal)?)?;
            file.sync_all()?;
        }
        fs::rename(temporary, journal_path)?;
        Ok(reservation)
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn safe_create_dirs(root: &std::path::Path, target: &std::path::Path) -> Result<()> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("Write outside corpus".into()))?;
    let mut path = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(Error::Invalid("Invalid write path".into()));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => (),
            Ok(_) => {
                return Err(Error::Invalid(
                    "Write path contains a symlink or non-directory".into(),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
