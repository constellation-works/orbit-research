use crate::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub use orbit_research_common::{Record, Snapshot};

pub struct Corpus {
    root: PathBuf,
    pub(crate) schema: Value,
}

impl Corpus {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let schema: Value = serde_json::from_slice(&fs::read(root.join("_scripts/schema.json"))?)?;
        if !schema["x-observatory"]["kinds"].is_object() {
            return Err(Error::Invalid(
                "Corpus does not export the Observatory record contract".into(),
            ));
        }
        Ok(Self { root, schema })
    }

    pub fn schema(&self) -> &Value {
        &self.schema
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&self.schema)
            .map_err(|e| Error::Invalid(format!("Invalid owner schema: {e}")))?;
        let mut records = BTreeMap::new();
        let kinds = self.schema["x-observatory"]["kinds"]
            .as_object()
            .ok_or_else(|| Error::Invalid("Missing record kinds".into()))?;
        for (kind, spec) in kinds {
            let directory = spec["directory"]
                .as_str()
                .ok_or_else(|| Error::Invalid("Missing record directory".into()))?;
            let base = self.safe_path(Path::new(directory))?;
            for entry in fs::read_dir(base)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
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
                let path = self.safe_path(&relative)?;
                let bytes = fs::read(&path)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::Invalid(format!("{} is not UTF-8", relative.display())))?;
                let (metadata, body) = parse(text)?;
                if let Err(errors) = validator.validate(&metadata) {
                    return Err(Error::Invalid(format!(
                        "{}: {}",
                        relative.display(),
                        errors.map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
                    )));
                }
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
                    git_blob: self.git(&[
                        "hash-object",
                        "--",
                        path.to_str()
                            .ok_or_else(|| Error::Invalid("Non UTF-8 path".into()))?,
                    ])?,
                };
                if records.insert(id.clone(), record).is_some() {
                    return Err(Error::Invalid(format!("Duplicate record ID: {id}")));
                }
            }
        }
        check_numbering(&records)?;
        for record in records.values() {
            for (field, allowed) in [
                ("derived_from", &["Q", "H", "T", "R"] as &[&str]),
                ("answered_by", &["H", "R"]),
                ("tests", &["H"]),
                ("claims", &["H"]),
                ("supersedes", &["T"]),
            ] {
                if let Some(refs) = record.metadata[field].as_array() {
                    for target in refs {
                        let target = target
                            .as_str()
                            .ok_or_else(|| Error::Invalid("Invalid reference".into()))?;
                        let Some(target_record) = records.get(target) else {
                            return Err(Error::Invalid(format!(
                                "{} {field} references missing {target}",
                                record.id
                            )));
                        };
                        if !allowed.contains(&target_record.kind.as_str()) {
                            return Err(Error::Invalid(format!(
                                "{} {field} references {} record {target}; expected {}",
                                record.id,
                                target_record.kind,
                                allowed.join(" or ")
                            )));
                        }
                    }
                }
            }
            if let Some(assessments) = record.metadata["assessments"].as_array() {
                for (index, assessment) in assessments.iter().enumerate() {
                    let target = assessment["research"].as_str().ok_or_else(|| {
                        Error::Invalid(format!(
                            "{} assessments[{index}] has invalid research reference",
                            record.id
                        ))
                    })?;
                    let Some(target_record) = records.get(target) else {
                        return Err(Error::Invalid(format!(
                            "{} assessments[{index}].research references missing {target}",
                            record.id
                        )));
                    };
                    if target_record.kind != "R" {
                        return Err(Error::Invalid(format!(
                            "{} assessments[{index}].research references {} record {target}; expected R",
                            record.id, target_record.kind
                        )));
                    }
                    if let (Some(current), Some(revision)) = (
                        record.metadata["revision"].as_u64(),
                        assessment["revision"].as_u64(),
                    ) && revision > current
                    {
                        return Err(Error::Invalid(format!(
                            "{} assessments[{index}] is against revision {revision}, beyond hypothesis revision {current}",
                            record.id
                        )));
                    }
                }
            }
            check_lineage(
                &record.id,
                &records,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            )?;
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
            revision: self.git(&["rev-parse", "HEAD"])?,
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

    pub fn published(&self, revision: &str, reference: &str) -> Result<bool> {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["merge-base", "--is-ancestor", revision, reference])
            .status()?;
        Ok(status.success())
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
            .arg("-C")
            .arg(&self.root)
            .args(["show", &format!("{revision}:{path}")])
            .output()?;
        if !out.status.success() {
            return Err(Error::Invalid("Missing committed evidence".into()));
        }
        Ok(out.stdout)
    }

    pub(crate) fn git(&self, args: &[&str]) -> Result<String> {
        let result = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()?;
        if !result.status.success() {
            return Err(Error::Invalid(
                String::from_utf8_lossy(&result.stderr).trim().into(),
            ));
        }
        Ok(String::from_utf8_lossy(&result.stdout).trim().into())
    }
}

fn parse(text: &str) -> Result<(Value, String)> {
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| Error::Invalid("Missing frontmatter".into()))?;
    let (front, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| Error::Invalid("Unclosed frontmatter".into()))?;
    Ok((serde_yaml::from_str(front)?, body.to_owned()))
}

fn parse_record_name(kind: &str, name: &str, directory_layout: bool) -> Result<(String, String)> {
    let suffix = if directory_layout {
        name
    } else {
        name.strip_suffix(".md")
            .ok_or_else(|| Error::Invalid(format!("Record filename must end in .md: {name}")))?
    };
    let (id, slug) = suffix
        .split_once('-')
        .ok_or_else(|| Error::Invalid(format!("Record path must be {kind}###-slug: {name}")))?;
    if id.len() != kind.len() + 3
        || !id.starts_with(kind)
        || !id[kind.len()..].bytes().all(|byte| byte.is_ascii_digit())
        || slug.is_empty()
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Error::Invalid(format!(
            "Record path must be {kind}###-slug: {name}"
        )));
    }
    Ok((id.to_owned(), slug.to_owned()))
}

fn kebab(title: &str) -> String {
    let mut slug = String::new();
    for byte in title.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            slug.push(byte as char);
        } else if byte.is_ascii_uppercase() {
            slug.push((byte + b'a' - b'A') as char);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn check_numbering(records: &BTreeMap<String, Record>) -> Result<()> {
    let mut numbers: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for record in records.values() {
        let number = record.id[1..]
            .parse::<u32>()
            .map_err(|_| Error::Invalid(format!("Invalid record ID: {}", record.id)))?;
        numbers.entry(record.kind.clone()).or_default().push(number);
    }
    for (kind, mut values) in numbers {
        values.sort_unstable();
        for (index, number) in values.into_iter().enumerate() {
            let expected = index as u32 + 1;
            if number != expected {
                return Err(Error::Invalid(format!(
                    "Non-monotonic IDs: expected {kind}{expected:03}, found {kind}{number:03}"
                )));
            }
        }
    }
    Ok(())
}

fn check_lineage(
    id: &str,
    records: &BTreeMap<String, Record>,
    visiting: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
) -> Result<()> {
    if done.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.into()) {
        return Err(Error::Invalid(format!("Lineage cycle at {id}")));
    }
    if let Some(parents) = records[id].metadata["derived_from"].as_array() {
        for parent in parents {
            let parent = parent
                .as_str()
                .ok_or_else(|| Error::Invalid("Invalid lineage ID".into()))?;
            check_lineage(parent, records, visiting, done)?;
        }
    }
    visiting.remove(id);
    done.insert(id.into());
    Ok(())
}
