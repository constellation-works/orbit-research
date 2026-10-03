use crate::{
    Error, Result, git,
    record::{kebab, parse, parse_record_name},
    validation::{Contract, validate_records},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub use orbit_research_common::{Record, Snapshot};

/// A record's location in an owner directory, named but not yet read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordEntry {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) slug: String,
    /// Corpus-relative `/` path of the record's Markdown file.
    pub(crate) path: String,
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
                    "Corpus path does not exist: {}",
                    requested_root.display()
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
                        "Corpus at {} is missing the corpus contract: {}",
                        root.display(),
                        schema_path.display()
                    ))
                }
                Error::Io(error) => Error::Invalid(format!(
                    "Unable to read the corpus contract at {}: {error}",
                    schema_path.display()
                )),
                error => error,
            })?;
        let corpus = Self {
            root,
            contract: Contract::compile(serde_json::from_slice(&schema_bytes)?)?,
        };
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
            let schema = serde_json::from_slice(&committed.bytes("_scripts/schema.json")?)?;
            if schema == self.contract.schema {
                self.read_snapshot(&self.contract, committed.revision(), Some(&committed))
            } else {
                let contract = Contract::compile(schema)?;
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
        let records = self.read_records(contract, committed)?;
        validate_records(&records)?;
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
    /// `committed` paths of one revision), without reading its contents.
    pub(crate) fn record_entries(
        &self,
        contract: &Contract,
        committed: Option<&[String]>,
    ) -> Result<Vec<RecordEntry>> {
        let mut entries = Vec::new();
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
                fs::read_dir(self.safe_path(Path::new(directory))?)?
                    .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect::<std::io::Result<_>>()?
            };
            for name in names {
                if !name.starts_with(kind) {
                    continue;
                }
                let (id, slug) = parse_record_name(kind, &name, spec["layout"] == "directory")?;
                let relative = if spec["layout"] == "directory" {
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
        Ok(entries)
    }

    /// Parse and schema-check every record, without whole-corpus graph checks.
    pub(crate) fn read_records(
        &self,
        contract: &Contract,
        committed: Option<&git::read::CommittedView<'_>>,
    ) -> Result<BTreeMap<String, Record>> {
        let mut records = BTreeMap::new();
        let paths = committed.map(|view| view.paths()).transpose()?;
        for entry in self.record_entries(contract, paths.as_deref())? {
            let bytes = if let Some(view) = committed {
                view.bytes(&entry.path)?
            } else {
                self.working_bytes(&entry.path)?
            };
            let record = self.decode_record(contract, &entry, &bytes)?;
            if records.insert(record.id.clone(), record).is_some() {
                return Err(Error::Invalid(format!("Duplicate record ID: {}", entry.id)));
            }
        }
        Ok(records)
    }

    /// One record from its bytes: frontmatter, owner schema, ID and frozen slug.
    pub(crate) fn decode_record(
        &self,
        contract: &Contract,
        entry: &RecordEntry,
        bytes: &[u8],
    ) -> Result<Record> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid(format!("{} is not UTF-8", entry.path)))?;
        let (metadata, body) = parse(text)?;
        contract.validate(&metadata, &entry.path)?;
        let id = metadata["id"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Missing id".into()))?
            .to_owned();
        if id != entry.id {
            return Err(Error::Invalid(format!(
                "Record ID/path mismatch: {}",
                entry.path
            )));
        }
        if metadata["slug"]
            .as_str()
            .is_some_and(|declared| declared != entry.slug)
            || metadata["slug"].is_null()
                && metadata["title"]
                    .as_str()
                    .is_some_and(|title| kebab(title) != entry.slug)
        {
            return Err(Error::Invalid(format!(
                "Record slug/path mismatch: {}",
                entry.path
            )));
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
