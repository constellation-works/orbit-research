//! Serialized writes with durable intents shared by allocation and question revision.
use crate::{Error, Result, corpus::Corpus, record};
use fs2::FileExt;
pub use orbit_research_common::Reservation;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
struct WriteFile {
    path: String,
    text: String,
    /// None means create only. Revisions retain exact original bytes for safe retry.
    before: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReservationIntent {
    request_digest: String,
    id: String,
    path: String,
    text: String,
    parent: String,
    reservation: Option<Reservation>,
    /// Older creation intents have only path/text. Upgrade them in memory on read.
    #[serde(default)]
    files: Vec<WriteFile>,
}

struct Writer<'a> {
    corpus: &'a Corpus,
    state: PathBuf,
    _lock: File,
}

impl Corpus {
    /// Caller retains the same key across uncertain responses.
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
        let title = checked_title(title)?;
        self.git(&["rev-parse", "HEAD"])?;
        let writer = Writer::open(self)?;
        let key = digest(request_key.as_bytes());
        let request_digest = digest(&serde_json::to_vec(&json!([
            kind,
            title,
            body,
            tags,
            derived_from
        ]))?);
        let intent = match writer.load(&key, &request_digest)? {
            Some(intent) => intent,
            None => {
                writer.require_clean()?;
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
                let (path, text) =
                    record::scaffold(&self.contract, &id, kind, title, body, tags, derived_from)?;
                let mut intent = ReservationIntent {
                    request_digest,
                    id,
                    path,
                    text,
                    parent: snapshot.revision,
                    reservation: None,
                    files: vec![],
                };
                intent.upgrade_files();
                writer.save_new(&key, &intent)?;
                intent
            }
        };
        writer.finish(&key, intent)
    }

    /// The expected blob and complete request identify a retry, including a failed commit.
    pub fn revise_question(
        &self,
        id: &str,
        expected_blob: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
    ) -> Result<Reservation> {
        let title = checked_title(title)?;
        let writer = Writer::open(self)?;
        let request_digest = digest(&serde_json::to_vec(&json!([
            id,
            expected_blob,
            title,
            body,
            tags
        ]))?);
        let key = format!("revision-{request_digest}");
        let intent = match writer.load(&key, &request_digest)? {
            Some(intent) => intent,
            None => {
                writer.require_clean()?;
                let snapshot = self.snapshot()?;
                let record = snapshot
                    .records
                    .iter()
                    .find(|r| r.id == id && r.kind == "Q")
                    .ok_or_else(|| Error::Invalid("Only existing questions are editable".into()))?;
                if record.git_blob != expected_blob {
                    return Err(Error::Conflict(
                        "Question changed since it was opened; reload before editing".into(),
                    ));
                }
                let before = fs::read_to_string(self.root().join(&record.path))?;
                if self.hash_bytes(before.as_bytes())? != expected_blob {
                    return Err(Error::Conflict(
                        "Question changed while preparing revision".into(),
                    ));
                }
                let filename = Path::new(&record.path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| Error::Invalid("Invalid question filename".into()))?;
                let mut metadata = record.metadata.clone();
                metadata["slug"] = json!(
                    filename
                        .split_once('-')
                        .ok_or_else(|| Error::Invalid("Missing frozen slug".into()))?
                        .1
                );
                metadata["title"] = json!(title);
                metadata["tags"] = json!(tags);
                metadata["updated"] = json!(record::utc_date()?);
                self.contract.validate(&metadata, &record.path)?;
                let text = record::render(&metadata, &format!("{body}\n"))?;
                let intent = ReservationIntent {
                    request_digest,
                    id: id.into(),
                    path: record.path.clone(),
                    text: text.clone(),
                    parent: snapshot.revision,
                    reservation: None,
                    files: vec![WriteFile {
                        path: record.path.clone(),
                        text,
                        before: Some(before),
                    }],
                };
                writer.save_new(&key, &intent)?;
                intent
            }
        };
        writer.finish(&key, intent)
    }
}

impl ReservationIntent {
    fn upgrade_files(&mut self) {
        if !self.files.is_empty() {
            return;
        }
        self.files.push(WriteFile {
            path: self.path.clone(),
            text: self.text.clone(),
            before: None,
        });
        if self.id.starts_with('R') {
            if let Some(parent) = Path::new(&self.path).parent() {
                self.files.push(WriteFile {
                    // Intent paths are Git paths, including on Windows.
                    path: format!("{}/data/manifest.json", parent.to_string_lossy()),
                    text: "{\"inputs\":[]}\n".into(),
                    before: None,
                });
            }
        }
    }
}

impl<'a> Writer<'a> {
    fn open(corpus: &'a Corpus) -> Result<Self> {
        let common = PathBuf::from(corpus.git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?);
        let own = PathBuf::from(corpus.git(&["rev-parse", "--absolute-git-dir"])?);
        if own.canonicalize()? != common.canonicalize()? {
            return Err(Error::Invalid("Writes require the primary integration checkout; reserve IDs before dispatching a worktree".into()));
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
        let schema: serde_json::Value =
            serde_json::from_slice(&fs::read(corpus.root().join("_scripts/schema.json"))?)?;
        if schema != *corpus.schema() {
            return Err(Error::Invalid(
                "Owner schema changed; reopen the corpus before writing".into(),
            ));
        }
        Ok(Self {
            corpus,
            state,
            _lock: lock,
        })
    }

    fn require_clean(&self) -> Result<()> {
        if !self.corpus.git(&["status", "--porcelain"])?.is_empty() {
            return Err(Error::Invalid(
                "Write requires a clean corpus integration checkout; preserve existing edits first"
                    .into(),
            ));
        }
        Ok(())
    }

    fn load(&self, key: &str, request_digest: &str) -> Result<Option<ReservationIntent>> {
        let path = self.state.join(format!("{key}.json"));
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut intent: ReservationIntent = serde_json::from_slice(&bytes).map_err(|e| {
            Error::Invalid(format!(
                "Incomplete write intent at {}; preserve it for reconciliation: {e}",
                path.display()
            ))
        })?;
        if intent.request_digest != request_digest {
            return Err(Error::Invalid(
                "Request key was already used for different content".into(),
            ));
        }
        intent.upgrade_files();
        Ok(Some(intent))
    }

    fn save_new(&self, key: &str, intent: &ReservationIntent) -> Result<()> {
        atomic_write(
            &self.state.join(format!("{key}.json")),
            &serde_json::to_vec(intent)?,
            false,
        )
    }

    fn finish(&self, key: &str, mut intent: ReservationIntent) -> Result<Reservation> {
        if let Some(receipt) = &intent.reservation {
            self.verify_files(&intent, &receipt.commit)?;
            return Ok(receipt.clone());
        }
        let head = self.corpus.git(&["rev-parse", "HEAD"])?;
        let commit = if head != intent.parent {
            self.recover_commit(key, &intent, &head)?;
            head
        } else {
            self.apply_files(&intent)?;
            self.corpus.snapshot()?;
            self.commit(key, &intent)?
        };
        self.verify_files(&intent, &commit)?;
        let reservation = Reservation {
            id: intent.id.clone(),
            path: intent.path.clone(),
            commit,
            request_digest: intent.request_digest.clone(),
        };
        intent.reservation = Some(reservation.clone());
        atomic_write(
            &self.state.join(format!("{key}.json")),
            &serde_json::to_vec(&intent)?,
            true,
        )?;
        Ok(reservation)
    }

    fn apply_files(&self, intent: &ReservationIntent) -> Result<()> {
        // Check every destination before changing any of them; retries never overwrite differing bytes.
        for file in &intent.files {
            self.check_file(file)?;
        }
        for file in &intent.files {
            let path = self.corpus.root().join(&file.path);
            let parent = path
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid write path".into()))?;
            safe_create_dirs(self.corpus.root(), parent)?;
            self.check_file(file)?;
            if fs::read(&path).ok().as_deref() == Some(file.text.as_bytes()) {
                continue;
            }
            atomic_write(&path, file.text.as_bytes(), file.before.is_some())?;
        }
        if intent.id.starts_with('R') {
            let parent = self
                .corpus
                .root()
                .join(&intent.path)
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid research path".into()))?
                .to_owned();
            for dir in ["code", "artifacts"] {
                safe_create_dirs(self.corpus.root(), &parent.join(dir))?;
            }
        }
        Ok(())
    }

    fn check_file(&self, file: &WriteFile) -> Result<()> {
        let relative = Path::new(&file.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::Invalid("Invalid write path".into()));
        }
        let mut path = self.corpus.root().to_owned();
        for component in relative.components() {
            path.push(component);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(Error::Invalid("Write path contains a symlink".into()));
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(e.into()),
            }
        }
        let path = self.corpus.root().join(relative);
        match fs::read(&path) {
            Ok(bytes)
                if bytes == file.text.as_bytes()
                    || file.before.as_ref().is_some_and(|s| bytes == s.as_bytes()) =>
            {
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && file.before.is_none() => Ok(()),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Err(Error::Conflict(format!(
                "Write path {} has conflicting edits; preserve intent for reconciliation",
                file.path
            ))),
        }
    }

    fn commit(&self, key: &str, intent: &ReservationIntent) -> Result<String> {
        if self.corpus.git(&["rev-parse", "HEAD"])? != intent.parent {
            return Err(Error::Conflict(
                "Corpus advanced before commit; reconcile incomplete write".into(),
            ));
        }
        let paths: Vec<&str> = intent.files.iter().map(|f| f.path.as_str()).collect();
        self.check_staged(intent)?;
        let mut args = vec!["add", "--"];
        args.extend(&paths);
        self.corpus.git(&args)?;
        self.check_staged(intent)?;
        if !self
            .corpus
            .git(&["diff", "--cached", "--name-only"])?
            .is_empty()
        {
            self.corpus.git(&[
                "commit",
                "-m",
                &format!(
                    "Write research record {}\n\nOrbit-Research-Request: {key}",
                    intent.id
                ),
            ])?;
        }
        self.corpus.git(&["rev-parse", "HEAD"])
    }

    fn check_staged(&self, intent: &ReservationIntent) -> Result<()> {
        let staged = self
            .corpus
            .git_bytes(&["diff", "--cached", "--name-only", "-z"])?;
        for path in staged.split(|b| *b == 0).filter(|b| !b.is_empty()) {
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
                && !file.before.as_ref().is_some_and(|s| bytes == s.as_bytes())
            {
                return Err(Error::Conflict(format!(
                    "Staged path {} has conflicting edits; refusing commit",
                    file.path
                )));
            }
        }
        Ok(())
    }

    fn recover_commit(&self, key: &str, intent: &ReservationIntent, head: &str) -> Result<()> {
        let parent = self.corpus.git(&["rev-parse", &format!("{head}^")])?;
        let message = self.corpus.git(&["log", "-1", "--format=%B", head])?;
        if parent != intent.parent
            || !message
                .lines()
                .any(|l| l == format!("Orbit-Research-Request: {key}"))
        {
            return Err(Error::Invalid(
                "Corpus advanced during incomplete reservation; manual reconciliation required"
                    .into(),
            ));
        }
        let changed =
            self.corpus
                .git(&["diff-tree", "--no-commit-id", "--name-only", "-r", head])?;
        if changed
            .lines()
            .any(|p| !intent.files.iter().any(|f| f.path == p))
        {
            return Err(Error::Invalid(
                "Recovered commit contains unrelated paths".into(),
            ));
        }
        self.verify_files(intent, head)
    }

    fn verify_files(&self, intent: &ReservationIntent, commit: &str) -> Result<()> {
        for file in &intent.files {
            if self.corpus.committed_bytes(commit, &file.path)? != file.text.as_bytes() {
                return Err(Error::Invalid(format!(
                    "Committed reservation content differs: {}",
                    file.path
                )));
            }
        }
        Ok(())
    }
}

fn checked_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::Invalid("Title is required".into()));
    }
    Ok(title)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Persist complete bytes before publishing the name. Sync directory entries where supported.
fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Missing parent directory".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if replace && path.exists() {
        temporary
            .as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if replace {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .map_err(|e| Error::Io(e.error))?;
    // std::fs::File::open cannot open Windows directories without
    // FILE_FLAG_BACKUP_SEMANTICS; std offers no portable directory sync.
    // Synced file contents and atomic publication still permit intent retries.
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
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
