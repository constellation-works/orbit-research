use crate::{
    Error, Result,
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

pub struct Corpus {
    root: PathBuf,
    pub(crate) contract: Contract,
}

impl Corpus {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let schema: Value = serde_json::from_slice(&fs::read(root.join("_scripts/schema.json"))?)?;
        Ok(Self {
            root,
            contract: Contract::compile(schema)?,
        })
    }

    pub fn schema(&self) -> &Value {
        &self.contract.schema
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validated working-tree view. `revision` is its base HEAD, not a content promise.
    pub fn snapshot(&self) -> Result<Snapshot> {
        let revision = self.git(&["rev-parse", "HEAD"])?;
        self.read_snapshot(&self.contract, &revision, None)
    }

    /// Immutable records and owner schema from one pinned commit, for work plans.
    pub fn committed_snapshot(&self) -> Result<Snapshot> {
        let revision = self.git(&["rev-parse", "--verify", "HEAD^{commit}"])?;
        let schema =
            serde_json::from_slice(&self.committed_bytes(&revision, "_scripts/schema.json")?)?;
        let paths = self.committed_paths(&revision)?;
        if schema == self.contract.schema {
            self.read_snapshot(&self.contract, &revision, Some(&paths))
        } else {
            let contract = Contract::compile(schema)?;
            self.read_snapshot(&contract, &revision, Some(&paths))
        }
    }

    fn read_snapshot(
        &self,
        contract: &Contract,
        revision: &str,
        committed: Option<&[String]>,
    ) -> Result<Snapshot> {
        let mut records = BTreeMap::new();
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
                let (expected_id, slug) =
                    parse_record_name(kind, &name, spec["layout"] == "directory")?;
                let relative = if spec["layout"] == "directory" {
                    Path::new(directory).join(&name).join("README.md")
                } else {
                    Path::new(directory).join(&name)
                };
                let bytes = if committed.is_some() {
                    self.committed_bytes(revision, &relative.to_string_lossy())?
                } else {
                    fs::read(self.safe_path(&relative)?)?
                };
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::Invalid(format!("{} is not UTF-8", relative.display())))?;
                let (metadata, body) = parse(text)?;
                contract.validate(&metadata, &relative.to_string_lossy())?;
                let id = metadata["id"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("Missing id".into()))?
                    .to_owned();
                if id != expected_id {
                    return Err(Error::Invalid(format!(
                        "Record ID/path mismatch: {}",
                        relative.display()
                    )));
                }
                if metadata["slug"]
                    .as_str()
                    .is_some_and(|declared| declared != slug)
                    || metadata["slug"].is_null()
                        && metadata["title"]
                            .as_str()
                            .is_some_and(|title| kebab(title) != slug)
                {
                    return Err(Error::Invalid(format!(
                        "Record slug/path mismatch: {}",
                        relative.display()
                    )));
                }
                let record = Record {
                    id: id.clone(),
                    kind: kind.clone(),
                    path: relative.to_string_lossy().into_owned(),
                    metadata,
                    body,
                    content_sha256: format!("{:x}", Sha256::digest(&bytes)),
                    git_blob: self.hash_bytes(&bytes)?,
                };
                if records.insert(id.clone(), record).is_some() {
                    return Err(Error::Invalid(format!("Duplicate record ID: {id}")));
                }
            }
        }
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

    fn safe_path(&self, relative: &Path) -> Result<PathBuf> {
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::Invalid(
                "Corpus path must be relative without traversal".into(),
            ));
        }
        let mut path = self.root.clone();
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
}
