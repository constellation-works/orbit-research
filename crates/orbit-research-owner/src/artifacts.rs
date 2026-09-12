//! Typed, opt-in owner verification of retained non-Git dataset snapshots.
//!
//! The owner supplies format/schema validation; this crate verifies the exact record,
//! content descriptor, byte members and parent pins. A capability rechecks on use: it is
//! not a durable attestation that can travel without the bytes it describes.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use orbit_research_contract::{
    ArtifactResolver, ContractError, Pin, Reference, VerifiedPin, canonical_json, parse_json,
    validate_structural,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{OwnerError, Result, require};

/// Owner-specific parsing (Arrow schema, rows, units, sidecars) lives with its owner.
pub trait SchemaCheck: Send + Sync {
    /// Read only; return an error if schema, units, sidecar or parent meaning differs.
    fn check(&self, root: &Path, record: &Value, descriptor: &Value) -> Result<()>;
}

/// A recheckable capability, not a serialized claim that external bytes are trusted.
#[derive(Clone)]
pub struct VerifiedArtifact {
    root: PathBuf,
    record_bytes: Vec<u8>,
    descriptor_bytes: Vec<u8>,
    byte_fields: Vec<(String, String)>,
    record_path: String,
    semantic_check: Arc<dyn SchemaCheck>,
}

impl VerifiedArtifact {
    /// The retained artifact record, as canonical bytes were supplied.
    pub fn record(&self) -> Result<Value> {
        Ok(parse_json(&self.record_bytes)?)
    }

    /// Re-verify record, descriptor and every byte member, then return the exact record.
    pub fn verify(&self) -> Result<Value> {
        let record = self.record()?;
        let descriptor = parse_json(&self.descriptor_bytes)?;
        require(
            validate_structural(&record).is_empty(),
            "invalid external artifact record",
        )?;
        let payload = record.get("payload").cloned().unwrap_or(Value::Null);
        require(
            text(&record, "kind") == Some("artifact")
                && text(&payload, "role") == Some("dataset")
                && text(&payload, "availability") == Some("available"),
            "only available dataset artifacts can use owner byte verification",
        )?;
        require(
            digest_bytes(&canonical_json(&descriptor)?)
                == text(&payload, "snapshot_digest").unwrap_or_default(),
            "snapshot descriptor digest mismatch",
        )?;
        require(
            descriptor.get("schema").is_some()
                && descriptor.get("parent_pins") == record.get("references"),
            "schema and exact parent pins must be retained in descriptor",
        )?;
        let declared: BTreeMap<_, _> = self.byte_fields.iter().cloned().collect();
        require(
            !self.byte_fields.is_empty() && declared.len() == self.byte_fields.len(),
            "unique descriptor byte fields required",
        )?;
        let expected: Vec<&String> = descriptor
            .as_object()
            .map(|object| {
                object
                    .keys()
                    .filter(|key| key.ends_with("_sha256"))
                    .collect()
            })
            .unwrap_or_default();
        require(
            declared.len() == expected.len()
                && expected.iter().all(|key| declared.contains_key(*key)),
            "every descriptor byte digest must be verified; no omitted data or sidecar member",
        )?;
        let record_path = member_path(&self.root, &self.record_path)?;
        require(
            canonical_json(&parse_json(&std::fs::read(&record_path)?)?)? == self.record_bytes,
            "retained artifact record was changed",
        )?;
        let mut members = Vec::new();
        for (field, relative) in &self.byte_fields {
            let digest = text(&descriptor, field)
                .ok_or_else(|| OwnerError::Invalid("byte member lacks descriptor digest".into()))?;
            let path = member_path(&self.root, relative)?;
            require(
                file_digest(&path)? == digest,
                &format!("artifact byte digest mismatch: {relative}"),
            )?;
            members.push((path, digest.to_owned()));
        }
        self.semantic_check
            .check(&self.root, &record, &descriptor)?;
        // Re-read after owner parsing: no claimed stable proof across a concurrent edit.
        require(
            canonical_json(&parse_json(&std::fs::read(&record_path)?)?)? == self.record_bytes,
            "artifact record changed during verification",
        )?;
        for (path, digest) in members {
            require(
                file_digest(&path)? == digest,
                "artifact bytes changed during verification",
            )?;
        }
        Ok(record)
    }
}

/// Verify an explicit immutable owner snapshot; never infer missing parent lineage.
pub fn verify_artifact(
    record: &Value,
    root: &Path,
    descriptor: &Value,
    byte_fields: &BTreeMap<String, String>,
    semantic_check: Arc<dyn SchemaCheck>,
    record_path: &str,
) -> Result<VerifiedArtifact> {
    let root = root
        .canonicalize()
        .map_err(|error| OwnerError::Invalid(format!("{}: {error}", root.display())))?;
    require(root.is_dir(), "artifact root must be a directory")?;
    let proof = VerifiedArtifact {
        root,
        record_bytes: canonical_json(record)?,
        descriptor_bytes: canonical_json(descriptor)?,
        byte_fields: byte_fields
            .iter()
            .map(|(field, relative)| (field.clone(), relative.clone()))
            .collect(),
        record_path: record_path.to_owned(),
        semantic_check,
    };
    proof.verify()?;
    Ok(proof)
}

/// An owner-supplied capability set, keyed by exact pin. Returns `None` for anything else.
pub trait ArtifactSource: Send + Sync {
    fn resolve(&self, reference: &Value) -> Result<Option<VerifiedArtifact>>;
}

/// The concrete filesystem resolver: exact retained snapshots, verified on every use.
#[derive(Clone, Default)]
pub struct FsArtifactResolver {
    proofs: Vec<VerifiedArtifact>,
}

impl FsArtifactResolver {
    pub fn new(proofs: Vec<VerifiedArtifact>) -> Self {
        Self { proofs }
    }

    fn find(&self, reference: &Value) -> Result<Option<(&VerifiedArtifact, Value)>> {
        for proof in &self.proofs {
            let record = proof.record()?;
            let provenance = record.get("provenance").cloned().unwrap_or(Value::Null);
            if text(&record, "id") == text(reference, "id")
                && text(&record, "revision_id") == text(reference, "revision_id")
                && text(&provenance, "repository") == text(reference, "repository")
                && provenance.get("git_revision") == reference.get("source_revision")
            {
                return Ok(Some((proof, record)));
            }
        }
        Ok(None)
    }
}

impl ArtifactSource for FsArtifactResolver {
    fn resolve(&self, reference: &Value) -> Result<Option<VerifiedArtifact>> {
        Ok(self.find(reference)?.map(|(proof, _)| proof.clone()))
    }
}

/// The contract-side seam: a verified pin, or `None` when these bytes are not retained here.
impl ArtifactResolver for FsArtifactResolver {
    fn verify(
        &self,
        _record: &Value,
        reference: &Reference,
    ) -> std::result::Result<Option<VerifiedPin>, ContractError> {
        let encoded = serde_json::to_value(reference).map_err(ContractError::Json)?;
        let found = self
            .find(&encoded)
            .map_err(|error| ContractError::Schema(error.to_string()))?;
        let Some((proof, _)) = found else {
            return Ok(None);
        };
        proof
            .verify()
            .map_err(|error| ContractError::Schema(error.to_string()))?;
        Ok(Some(VerifiedPin(Pin {
            repository: reference.pin.repository.clone(),
            id: reference.pin.id.clone(),
            revision_id: reference.pin.revision_id.clone(),
            source_revision: reference.pin.source_revision.clone(),
        })))
    }
}

/// Artifact members stay beneath their explicit root and cannot use symlinks.
fn member_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    require(
        !candidate.is_absolute()
            && candidate
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
            && !candidate
                .components()
                .any(|component| matches!(component, Component::Normal(part) if part == "..")),
        "artifact member must stay under its explicit root",
    )?;
    let mut current = root.to_path_buf();
    for component in candidate.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            require(
                !current
                    .symlink_metadata()
                    .is_ok_and(|data| data.file_type().is_symlink()),
                "artifact member cannot be a symlink",
            )?;
        }
    }
    require(
        current
            .canonicalize()
            .is_ok_and(|path| path.starts_with(root))
            && current.is_file(),
        &format!("missing artifact member: {relative}"),
    )?;
    Ok(current)
}

fn file_digest(path: &Path) -> Result<String> {
    Ok(digest_bytes(&std::fs::read(path)?))
}

pub(crate) fn digest_bytes(data: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(data))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
