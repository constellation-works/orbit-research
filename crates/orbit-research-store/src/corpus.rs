use crate::{
    Error, Result, git,
    record::{kebab, parse, parse_record_name},
    validation::{Contract, validate_readable},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub use orbit_research_common::{CorpusIssue, Record, Snapshot};

/// A record's location in an owner directory, named but not yet read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordEntry {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) slug: String,
    /// Corpus-relative `/` path of the record's Markdown file.
    pub(crate) path: String,
}

/// The outcome of reading every record without stopping at the first problem.
struct LenientRead {
    records: BTreeMap<String, Record>,
    issues: Vec<CorpusIssue>,
    /// Ids of records that could not be read at all.
    unreadable: BTreeSet<String>,
    /// Ids of records in `records` that break the owner schema.
    flawed: BTreeSet<String>,
}

pub struct Corpus {
    root: PathBuf,
    pub(crate) contract: Contract,
}

impl Corpus {
    /// Open the selected corpus, refusing symlinked schema files or ancestors.
    pub fn open(root: &Path) -> Result<Self> {
        let requested_root = root.to_owned();
        let root = root.canonicalize().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::Invalid(format!(
                    "No research corpus at {path}: the path does not exist. Create one with `orbit-research workspace init {path}`",
                    path = requested_root.display()
                ))
            } else {
                Error::Invalid(format!(
                    "Unable to access corpus path {}: {error}",
                    requested_root.display()
                ))
            }
        })?;
        let schema_path = root.join("_scripts/schema.json");
        let schema_bytes = safe_path(&root, Path::new("_scripts/schema.json"))
            .and_then(|path| Ok(fs::read(path)?))
            .map_err(|error| match error {
                Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Error::Invalid(format!(
                        "No research corpus at {}: {} is missing. To start one, run `orbit-research workspace init {}` (it needs a new or empty directory)",
                        root.display(),
                        schema_path.display(),
                        root.display()
                    ))
                }
                Error::Io(error) => Error::Invalid(format!(
                    "Unable to read the corpus contract at {}: {error}",
                    schema_path.display()
                )),
                error => error,
            })?;
        let contract = Contract::from_bytes(&schema_bytes)
            .map_err(|error| in_file(error, &schema_path.display().to_string()))?;
        let corpus = Self { root, contract };
        corpus.ensure_repository()?;
        Ok(corpus)
    }

    pub fn schema(&self) -> &Value {
        &self.contract.schema
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validated working-tree view. `revision` is its base HEAD, not a content promise.
    pub fn snapshot(&self) -> Result<Snapshot> {
        let revision = self.head_commit()?;
        self.read_snapshot(&self.contract, &revision, None)
    }

    /// Immutable records and owner schema from one pinned commit, for work plans.
    pub fn committed_snapshot(&self) -> Result<Snapshot> {
        git::read::with_head(self.root(), |committed| {
            let bytes = committed.bytes("_scripts/schema.json")?;
            let label = format!("_scripts/schema.json at {}", committed.revision());
            let contract = Contract::from_bytes(&bytes).map_err(|error| in_file(error, &label))?;
            if contract.schema == self.contract.schema {
                self.read_snapshot(&self.contract, committed.revision(), Some(&committed))
            } else {
                self.read_snapshot(&contract, committed.revision(), Some(&committed))
            }
        })
    }

    /// Every file path in HEAD's tree, for checks about what a commit holds
    /// beyond its records (for example a contribution's findings).
    pub fn committed_paths(&self) -> Result<Vec<String>> {
        git::read::with_head(self.root(), |committed| committed.paths())
    }

    fn read_snapshot(
        &self,
        contract: &Contract,
        revision: &str,
        committed: Option<&git::read::CommittedView<'_>>,
    ) -> Result<Snapshot> {
        // Whole-corpus rules run over the records that did read, so a reference
        // or numbering problem is reported in the same pass as record problems.
        let LenientRead {
            records,
            mut issues,
            unreadable,
            flawed,
        } = self.read_records_lenient(contract, committed)?;
        if let Err(Error::Corpus(more)) = validate_readable(&records, &unreadable, &flawed) {
            issues.extend(more);
        }
        if !issues.is_empty() {
            return Err(Error::Corpus(issues));
        }
        let tags = records
            .values()
            .flat_map(|r| {
                r.metadata["tags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(Snapshot {
            revision: revision.to_owned(),
            records: records.into_values().collect(),
            tags,
        })
    }

    /// Every record entry named in the owner directories (working tree, or the
    /// `committed` paths of one revision), without reading its contents. A name
    /// that is not a record name is an error naming it.
    pub(crate) fn record_entries(
        &self,
        contract: &Contract,
        committed: Option<&[String]>,
    ) -> Result<Vec<RecordEntry>> {
        let (entries, issues, _) = self.scan_entries(contract, committed)?;
        if issues.is_empty() {
            Ok(entries)
        } else {
            Err(Error::Corpus(issues))
        }
    }

    /// The well-named entries, plus one issue for each name that is not a
    /// record name or each owner directory that is missing, and the ids such
    /// badly named entries start with (they exist, but cannot be read).
    fn scan_entries(
        &self,
        contract: &Contract,
        committed: Option<&[String]>,
    ) -> Result<(Vec<RecordEntry>, Vec<CorpusIssue>, BTreeSet<String>)> {
        let mut entries = Vec::new();
        let mut issues = Vec::new();
        let mut misnamed = BTreeSet::new();
        let kinds = contract.schema["x-observatory"]["kinds"]
            .as_object()
            .ok_or_else(|| Error::Invalid("Missing record kinds".into()))?;
        for (kind, spec) in kinds {
            let directory = spec["directory"]
                .as_str()
                .ok_or_else(|| Error::Invalid("Missing record directory".into()))?;
            let names: BTreeSet<String> = if let Some(paths) = committed {
                paths
                    .iter()
                    .filter_map(|path| path.strip_prefix(&format!("{directory}/")))
                    .filter_map(|path| path.split('/').next())
                    .map(str::to_owned)
                    .collect()
            } else {
                let listing = self
                    .safe_path(Path::new(directory))
                    .and_then(|path| Ok(fs::read_dir(path)?));
                match listing {
                    Ok(listing) => listing
                        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
                        .collect::<std::io::Result<_>>()?,
                    Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                        issues.push(CorpusIssue {
                            path: format!("{directory}/"),
                            field: None,
                            message: "the directory is missing; create it (an empty directory holding a .gitkeep file is enough)".into(),
                        });
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            };
            for name in names {
                if !name.starts_with(kind) {
                    continue;
                }
                let layout_is_directory = spec["layout"] == "directory";
                let (id, slug) = match parse_record_name(kind, &name, layout_is_directory) {
                    Ok(parsed) => parsed,
                    Err(message) => {
                        issues.push(CorpusIssue {
                            path: format!("{directory}/{name}"),
                            field: None,
                            message,
                        });
                        if let Some(id) = name.get(..4).filter(|id| {
                            id.starts_with(kind.as_str())
                                && id[1..].bytes().all(|byte| byte.is_ascii_digit())
                        }) {
                            misnamed.insert(id.to_owned());
                        }
                        continue;
                    }
                };
                let relative = if layout_is_directory {
                    Path::new(directory).join(&name).join("README.md")
                } else {
                    Path::new(directory).join(&name)
                };
                let path = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                entries.push(RecordEntry {
                    id,
                    kind: kind.clone(),
                    slug,
                    path,
                });
            }
        }
        Ok((entries, issues, misnamed))
    }

    /// Parse and schema-check every record, without whole-corpus graph checks.
    /// Every unreadable record is reported together, not just the first.
    pub(crate) fn read_records(
        &self,
        contract: &Contract,
        committed: Option<&git::read::CommittedView<'_>>,
    ) -> Result<BTreeMap<String, Record>> {
        let read = self.read_records_lenient(contract, committed)?;
        if read.issues.is_empty() {
            Ok(read.records)
        } else {
            Err(Error::Corpus(read.issues))
        }
    }

    /// Read every record, keeping what could be read when some cannot. A record
    /// whose frontmatter parses but breaks the contract still appears among the
    /// records (and in `flawed`), so its references are checked with the rest;
    /// no caller may use such a record once any problem is returned.
    fn read_records_lenient(
        &self,
        contract: &Contract,
        committed: Option<&git::read::CommittedView<'_>>,
    ) -> Result<LenientRead> {
        let mut flawed = BTreeSet::new();
        let mut records = BTreeMap::new();
        let paths = committed.map(|view| view.paths()).transpose()?;
        let (entries, mut issues, mut unreadable) =
            self.scan_entries(contract, paths.as_deref())?;
        for entry in entries {
            let bytes = if let Some(view) = committed {
                view.bytes(&entry.path)?
            } else {
                self.working_bytes(&entry.path)?
            };
            let record = match self.decode_record(contract, &entry, &bytes) {
                Ok(record) => record,
                Err(Error::Corpus(found)) => {
                    issues.extend(found);
                    match partial_record(&entry, &bytes) {
                        Some(partial) if !records.contains_key(&entry.id) => {
                            flawed.insert(entry.id.clone());
                            records.insert(entry.id.clone(), partial);
                        }
                        _ => {
                            unreadable.insert(entry.id);
                        }
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(first) = records
                .get(&record.id)
                .map(|first: &Record| first.path.clone())
            {
                issues.push(CorpusIssue {
                    path: entry.path,
                    field: Some("id".into()),
                    message: format!(
                        "{} is already used by {first}; ids must be unique",
                        entry.id
                    ),
                });
                continue;
            }
            records.insert(record.id.clone(), record);
        }
        Ok(LenientRead {
            records,
            issues,
            unreadable,
            flawed,
        })
    }

    /// One record from its bytes: frontmatter, owner schema, ID and frozen slug.
    pub(crate) fn decode_record(
        &self,
        contract: &Contract,
        entry: &RecordEntry,
        bytes: &[u8],
    ) -> Result<Record> {
        let problem = |field: Option<&str>, message: String| {
            Error::Corpus(vec![CorpusIssue {
                path: entry.path.clone(),
                field: field.map(str::to_owned),
                message,
            }])
        };
        let text = std::str::from_utf8(bytes)
            .map_err(|_| problem(None, "the file is not valid UTF-8 text".into()))?;
        let (metadata, body) = parse(&entry.path, text)?;
        contract.validate(&metadata, &entry.path, &entry.kind)?;
        let id = metadata["id"]
            .as_str()
            .ok_or_else(|| problem(Some("id"), "required field is missing".into()))?
            .to_owned();
        if id != entry.id {
            return Err(problem(
                Some("id"),
                format!(
                    "the id is {id} but the file name says {}; make them agree",
                    entry.id
                ),
            ));
        }
        if let Some(declared) = metadata["slug"]
            .as_str()
            .filter(|declared| *declared != entry.slug)
        {
            return Err(problem(
                Some("slug"),
                format!(
                    "the slug is \"{declared}\" but the path uses \"{}\"; the slug is frozen when the record is created, so change the field to match the path",
                    entry.slug
                ),
            ));
        }
        if metadata["slug"].is_null()
            && let Some(title) = metadata["title"].as_str()
            && kebab(title) != entry.slug
        {
            return Err(problem(
                Some("title"),
                format!(
                    "the title gives the path slug \"{}\" but the path uses \"{}\"; keep the path and add `slug: {}` to the frontmatter (the slug is frozen when the record is created)",
                    kebab(title),
                    entry.slug,
                    entry.slug
                ),
            ));
        }
        Ok(Record {
            id,
            kind: entry.kind.clone(),
            path: entry.path.clone(),
            metadata,
            body,
            content_sha256: format!("{:x}", Sha256::digest(bytes)),
            git_blob: self.hash_bytes(bytes)?,
        })
    }

    /// Working-tree bytes at a corpus-relative `/` path, refusing traversal and symlinks.
    pub(crate) fn working_bytes(&self, path: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.safe_path(Path::new(path))?)?)
    }

    pub(crate) fn safe_path(&self, relative: &Path) -> Result<PathBuf> {
        safe_path(&self.root, relative)
    }
}

/// What a record that breaks the contract still says: its parsed frontmatter and
/// body, so the references it makes can be checked. `None` when it does not parse.
fn partial_record(entry: &RecordEntry, bytes: &[u8]) -> Option<Record> {
    let (metadata, body) = parse(&entry.path, std::str::from_utf8(bytes).ok()?).ok()?;
    Some(Record {
        id: entry.id.clone(),
        kind: entry.kind.clone(),
        path: entry.path.clone(),
        metadata,
        body,
        content_sha256: String::new(),
        git_blob: String::new(),
    })
}

/// A contract problem tied to the file it came from, so the reader knows which
/// file to fix. Other errors (for example I/O) already carry their own context.
fn in_file(error: Error, file: &str) -> Error {
    match error {
        Error::Invalid(message) => Error::Invalid(format!("{file}: {message}")),
        error => error,
    }
}

/// Resolve working-tree paths before a contract exists as well as after opening.
fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid(
            "Corpus path must be relative without traversal".into(),
        ));
    }
    let mut path = root.to_owned();
    for component in relative.components() {
        path.push(component);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::Invalid(format!(
                "Corpus symlink refused: {}",
                relative.display()
            )));
        }
    }
    Ok(path)
}
