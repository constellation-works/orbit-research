//! Serialized primary-mode writes with durable intents shared by allocation,
//! revision and assessment, plus the mode every write runs in.
use crate::{
    Error, Result,
    corpus::Corpus,
    edit::{self, Assessment, Edit},
    record,
    worktree::WorktreeWrite,
};
use fs2::FileExt;
pub use orbit_research_common::Reservation;
use orbit_research_common::{Record, Snapshot};
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

/// Where a write runs, detected from the checkout's Git directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    /// The primary checkout: allocates IDs and commits under the writer lock.
    Primary,
    /// A linked run worktree: writes only its reserved R and never commits.
    Worktree,
}

impl std::fmt::Display for WriteMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Primary => "primary",
            Self::Worktree => "worktree",
        })
    }
}

/// A write result tagged with the mode that produced it.
#[derive(Debug, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WriteOutcome {
    /// Committed on the primary checkout.
    Primary(Reservation),
    /// Written in a run worktree and left for the run's commit step.
    Worktree(WorktreeWrite),
}

/// Dirty paths listed in a refusal before `+N more`.
const DIRTY_PATH_LIMIT: usize = 10;

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
            return Err(Error::InvalidInput(
                "The request key must be 1-256 bytes long; use a short, non-empty identifier of your own and reuse it only to retry the same request".into(),
            ));
        }
        if !matches!(kind, "Q" | "H" | "T" | "R") {
            return Err(Error::InvalidInput(
                "Record kind must be Q, H, T or R".into(),
            ));
        }
        let title = edit::checked_title(title)?;
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
                        return Err(Error::InvalidInput(format!(
                            "Unknown predecessor {parent}: it is not in the corpus; list the corpus to see the ids that exist"
                        )));
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
                intent.normalize_paths();
                intent.upgrade_files();
                writer.save_new(&key, &intent)?;
                intent
            }
        };
        writer.finish(&key, intent)
    }

    /// Revise a question; an omitted field keeps its current value. Kept for
    /// the `revise_question` operation; its retry identity predates `revise`
    /// and is unchanged for a request that names every field.
    pub fn revise_question(
        &self,
        id: &str,
        expected_blob: &str,
        title: Option<&str>,
        body: Option<&str>,
        tags: Option<Vec<String>>,
    ) -> Result<Reservation> {
        if let Some(title) = title {
            edit::checked_title(title)?;
        }
        let request_digest = digest(&serde_json::to_vec(&json!([
            id,
            expected_blob,
            title,
            body,
            tags
        ]))?);
        let edit = Edit {
            title: title.map(Into::into),
            body: body.map(Into::into),
            tags,
            ..Edit::default()
        };
        self.rewrite(
            format!("revision-{request_digest}"),
            request_digest,
            id,
            expected_blob,
            |_, record| {
                if record.kind != "Q" {
                    let elsewhere = if record.kind == "R" {
                        "a research item, which only its own run writes".to_owned()
                    } else {
                        format!(
                            "a {}; edit it with `research revise --id {id}`",
                            match record.kind.as_str() {
                                "H" => "hypothesis",
                                "T" => "theory",
                                other => other,
                            }
                        )
                    };
                    return Err(Error::InvalidInput(format!(
                        "revise-question edits questions only, and {id} is {elsewhere}"
                    )));
                }
                edit::revise(&self.contract, record, &edit, &record::utc_date()?)
            },
        )
    }

    /// Primary mode revises Q/H/T and commits; worktree mode writes only the
    /// worktree's reserved R and leaves it uncommitted.
    pub fn revise(&self, id: &str, expected_blob: &str, edit: &Edit) -> Result<WriteOutcome> {
        if self.write_mode()? == WriteMode::Worktree {
            return Ok(WriteOutcome::Worktree(self.write_reserved(
                id,
                expected_blob,
                edit,
            )?));
        }
        let request_digest = digest(&serde_json::to_vec(&json!([
            "revise",
            id,
            expected_blob,
            edit
        ]))?);
        let reservation = self.rewrite(
            format!("revision-{request_digest}"),
            request_digest,
            id,
            expected_blob,
            |_, record| {
                if record.kind == "R" {
                    return Err(Error::Refused(
                        "Research results are written by their run in worktree mode; primary-mode revise covers Q, H and T".into(),
                    ));
                }
                edit::revise(&self.contract, record, edit, &record::utc_date()?)
            },
        )?;
        Ok(WriteOutcome::Primary(reservation))
    }

    /// Append an assessment to a hypothesis (primary mode only). `accepted`
    /// sees the checkout's snapshot, clean at HEAD by then, and refuses
    /// research results without acceptance evidence.
    pub fn assess(
        &self,
        id: &str,
        expected_blob: &str,
        assessment: &Assessment,
        accepted: impl FnOnce(&Snapshot) -> Result<()>,
    ) -> Result<Reservation> {
        let request_digest = digest(&serde_json::to_vec(&json!([
            "assess",
            id,
            expected_blob,
            assessment
        ]))?);
        self.rewrite(
            format!("assessment-{request_digest}"),
            request_digest,
            id,
            expected_blob,
            |snapshot, record| {
                let text = edit::assess(
                    &self.contract,
                    &snapshot.records,
                    record,
                    assessment,
                    &record::utc_date()?,
                )?;
                // A stale blob is refused after these local checks and before
                // Orbit is asked, so an unreachable Orbit never masks it.
                if record.git_blob == expected_blob {
                    accepted(snapshot)?;
                }
                Ok(text)
            },
        )
    }

    /// Rewrite one existing record under its expected blob. The key covers the
    /// complete request, so an identical retry resumes or adopts the first result.
    fn rewrite(
        &self,
        key: String,
        request_digest: String,
        id: &str,
        expected_blob: &str,
        render: impl FnOnce(&Snapshot, &Record) -> Result<String>,
    ) -> Result<Reservation> {
        let writer = Writer::open(self)?;
        let intent = match writer.load(&key, &request_digest)? {
            Some(intent) => intent,
            None => {
                writer.require_clean()?;
                let snapshot = self.snapshot()?;
                let record = snapshot
                    .records
                    .iter()
                    .find(|r| r.id == id)
                    .ok_or_else(|| Error::NotFound(format!("Unknown research record id: {id}")))?;
                if record.git_blob != expected_blob {
                    // Render first so a wrong-kind request reports that, not staleness.
                    render(&snapshot, record)?;
                    return Err(Error::Conflict(format!(
                        "{id} has changed since you read it; run `research show --id {id}` again and use its `git_blob` as the expected blob"
                    )));
                }
                let before = fs::read_to_string(self.root().join(&record.path))?;
                if self.hash_bytes(before.as_bytes())? != expected_blob {
                    return Err(Error::Conflict(format!(
                        "{id} changed while the revision was being prepared; run `research show --id {id}` again and use its `git_blob` as the expected blob"
                    )));
                }
                let text = render(&snapshot, record)?;
                edit::check_records(&self.contract, &snapshot.records, &record.path, &text)?;
                let mut intent = ReservationIntent {
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
                intent.normalize_paths();
                writer.save_new(&key, &intent)?;
                intent
            }
        };
        writer.finish(&key, intent)
    }
}

impl ReservationIntent {
    fn normalize_paths(&mut self) {
        self.path = self.path.replace('\\', "/");
        for file in &mut self.files {
            file.path = file.path.replace('\\', "/");
        }
        if let Some(reservation) = &mut self.reservation {
            reservation.path = reservation.path.replace('\\', "/");
        }
    }

    fn upgrade_files(&mut self) {
        if !self.files.is_empty() {
            return;
        }
        self.files.push(WriteFile {
            path: self.path.clone(),
            text: self.text.clone(),
            before: None,
        });
        if self.id.starts_with('R')
            && let Some((parent, _)) = self.path.rsplit_once('/')
        {
            self.files.push(WriteFile {
                // Intent paths are Git paths, including on Windows.
                path: format!("{parent}/data/manifest.json"),
                text: "{\"inputs\":[]}\n".into(),
                before: None,
            });
        }
    }
}

impl<'a> Writer<'a> {
    fn open(corpus: &'a Corpus) -> Result<Self> {
        let (own, common) = corpus.git_dirs()?;
        if own != common {
            return Err(Error::Refused(
                "ID allocation and commits require the primary integration checkout; this linked worktree is in worktree mode, which writes only its reserved research record (reserve IDs before dispatching a worktree)".into(),
            ));
        }
        let state = common.join("orbit-research-writer");
        fs::create_dir_all(&state)?;
        let lock = exclusive_lock(&state.join("lock"))?;
        corpus.require_open_schema()?;
        Ok(Self {
            corpus,
            state,
            _lock: lock,
        })
    }

    fn require_clean(&self) -> Result<()> {
        let status = self.corpus.git_bytes(&["status", "--porcelain"])?;
        if status.is_empty() {
            return Ok(());
        }
        let status = String::from_utf8_lossy(&status);
        let dirty: Vec<&str> = status.lines().collect();
        let mut message = format!(
            "Write requires a clean corpus integration checkout; {} path{} with uncommitted changes in {}:",
            dirty.len(),
            if dirty.len() == 1 { "" } else { "s" },
            self.corpus.root().display()
        );
        for line in dirty.iter().take(DIRTY_PATH_LIMIT) {
            message.push_str("\n  ");
            message.push_str(line);
        }
        if dirty.len() > DIRTY_PATH_LIMIT {
            message.push_str(&format!("\n  +{} more", dirty.len() - DIRTY_PATH_LIMIT));
        }
        message.push_str("\nCommit, stash or remove them first, then retry.");
        if dirty
            .iter()
            .any(|line| line.contains(".orbit-research-tmp"))
        {
            message.push_str(&format!(
                " `.orbit-research-tmp/` is scratch left by an interrupted `accept`: delete it, and run `orbit-research workspace prepare-operations {}` so Git ignores it from now on.",
                self.corpus.root().display()
            ));
        }
        Err(Error::Invalid(message))
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
                "This request key was already used for different content; omit the request key to let one be generated, or choose a new key (reuse a key only to retry the identical request)".into(),
            ));
        }
        intent.normalize_paths();
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
            return Ok(Reservation {
                git_blob: self.corpus.hash_bytes(intent.text.as_bytes())?,
                replayed: true,
                ..receipt.clone()
            });
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
            changed: commit != intent.parent,
            commit,
            request_digest: intent.request_digest.clone(),
            git_blob: self.corpus.hash_bytes(intent.text.as_bytes())?,
            replayed: false,
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
        refuse_unsafe_path(self.corpus.root(), &file.path)?;
        let path = self.corpus.root().join(&file.path);
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
                && file.before.as_ref().is_none_or(|s| bytes != s.as_bytes())
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

impl Corpus {
    /// Primary checkout or linked run worktree; every write runs in exactly one.
    pub fn write_mode(&self) -> Result<WriteMode> {
        let (own, common) = self.git_dirs()?;
        Ok(if own == common {
            WriteMode::Primary
        } else {
            WriteMode::Worktree
        })
    }

    /// This checkout's own Git directory and the repository's common one.
    pub(crate) fn git_dirs(&self) -> Result<(PathBuf, PathBuf)> {
        let common = PathBuf::from(self.git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?);
        let own = PathBuf::from(self.git(&["rev-parse", "--absolute-git-dir"])?);
        Ok((own.canonicalize()?, common.canonicalize()?))
    }

    /// Writers reuse the compiled owner contract; a changed schema needs a reopen.
    pub(crate) fn require_open_schema(&self) -> Result<()> {
        let schema: serde_json::Value =
            serde_json::from_slice(&self.working_bytes("_scripts/schema.json")?).map_err(
                |error| Error::Invalid(format!("_scripts/schema.json is not valid JSON: {error}")),
            )?;
        if schema != *self.schema() {
            return Err(Error::Invalid(
                "Owner schema changed; reopen the corpus before writing".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn exclusive_lock(path: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    lock.lock_exclusive()?;
    Ok(lock)
}

/// Refuse absolute, traversing or symlinked write paths below the corpus root.
pub(crate) fn refuse_unsafe_path(root: &Path, relative: &str) -> Result<()> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid("Invalid write path".into()));
    }
    let mut path = root.to_owned();
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
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Persist complete bytes before publishing the name. Sync directory entries where supported.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
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

pub(crate) fn safe_create_dirs(root: &std::path::Path, target: &std::path::Path) -> Result<()> {
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
