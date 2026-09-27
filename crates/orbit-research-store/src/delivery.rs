//! Delivery gate: validate one research record at a checkout path before a run
//! commits it. Everything here reads in-process (no Git or clock subprocess),
//! so the sandboxed plugin can run it; nothing here writes.
use crate::{
    Error, Result,
    corpus::{Corpus, Record, RecordEntry},
    edit, git, record,
    validation::validate_records,
    worktree::BINDING,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Why a delivered research record fails the gate. Serialized as the plugin
/// error code, so each variant is a stable wire name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No research record to check: no `research_id` and no worktree binding,
    /// or the named id is not an R at the checkout's HEAD.
    ResearchUnbound,
    /// The record does not parse or fails the owner schema, ID or slug rules.
    RecordInvalid,
    /// A required README section is absent or out of order.
    SectionMissing,
    /// A required README section is empty or still holds the stub's text.
    SectionPlaceholder,
    /// `orbit.task` does not name the run's task.
    OrbitTaskMismatch,
    /// `orbit.run` does not name the run.
    OrbitRunMismatch,
    /// `data/manifest.json` is absent, not JSON or fails the owner schema.
    ManifestInvalid,
    /// A local input's bytes do not match its manifest `sha256` or `size`.
    ArtifactDigestMismatch,
    /// A lineage or reference target does not exist in the checkout.
    DanglingLineage,
    /// The checkout names a record absent from its HEAD: an allocated ID.
    IdAllocated,
    /// A record other than the research record changed or disappeared.
    OtherRecordChanged,
    /// The rest of the corpus fails the checker's whole-corpus rules.
    CorpusInvalid,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResearchUnbound => "research_unbound",
            Self::RecordInvalid => "record_invalid",
            Self::SectionMissing => "section_missing",
            Self::SectionPlaceholder => "section_placeholder",
            Self::OrbitTaskMismatch => "orbit_task_mismatch",
            Self::OrbitRunMismatch => "orbit_run_mismatch",
            Self::ManifestInvalid => "manifest_invalid",
            Self::ArtifactDigestMismatch => "artifact_digest_mismatch",
            Self::DanglingLineage => "dangling_lineage",
            Self::IdAllocated => "id_allocated",
            Self::OtherRecordChanged => "other_record_changed",
            Self::CorpusInvalid => "corpus_invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub reason: Reason,
    pub message: String,
}

/// What the delivered record must name. `research_id` selects the record
/// explicitly; without it, a run worktree's writer binding does.
#[derive(Debug, Clone, Copy)]
pub struct Expected<'a> {
    pub research_id: Option<&'a str>,
    pub task: &'a str,
    pub run: &'a str,
}

/// The gate's verdict. It passes only when `findings` is empty.
#[derive(Debug, Serialize)]
pub struct DeliveryReport {
    /// The checked research record, when one was identified.
    pub research_id: Option<String>,
    pub path: Option<String>,
    /// Git blob of the checked README bytes.
    pub blob: Option<String>,
    /// The checkout's HEAD commit.
    pub revision: String,
    pub findings: Vec<Finding>,
    /// Manifest inputs with a declared digest whose bytes are not in the
    /// checkout (data bytes are never committed), so were not verified.
    pub unverified_inputs: Vec<String>,
}

impl DeliveryReport {
    pub fn valid(&self) -> bool {
        self.findings.is_empty()
    }
}

impl Corpus {
    /// Validate one research record in this checkout's working tree against
    /// its HEAD. Reads only this checkout; an `Err` means the checkout itself
    /// could not be read, not that the record failed.
    pub fn validate_delivery(&self, expected: &Expected<'_>) -> Result<DeliveryReport> {
        let head = self.committed_snapshot()?;
        let mut report = DeliveryReport {
            research_id: None,
            path: None,
            blob: None,
            revision: head.revision.clone(),
            findings: Vec::new(),
            unverified_inputs: Vec::new(),
        };
        let Some(id) = self.delivered_id(expected, &mut report)? else {
            return Ok(report);
        };
        let Some(reserved) = head.records.iter().find(|r| r.id == id && r.kind == "R") else {
            report.fail(
                Reason::ResearchUnbound,
                format!("{id} is not a research record at the checkout's HEAD"),
            );
            return Ok(report);
        };
        report.research_id = Some(id.clone());
        report.path = Some(reserved.path.clone());

        let entries = self.record_entries(&self.contract, None)?;
        let entry = entries
            .iter()
            .find(|entry| entry.path == reserved.path)
            .cloned();
        let record = match entry.map(|entry| {
            self.working_bytes(&entry.path)
                .and_then(|bytes| self.decode_record(&self.contract, &entry, &bytes))
        }) {
            Some(Ok(record)) => record,
            Some(Err(error)) => {
                report.fail(Reason::RecordInvalid, error.to_string());
                return Ok(report);
            }
            None => {
                report.fail(
                    Reason::RecordInvalid,
                    format!("{} is missing from the checkout", reserved.path),
                );
                return Ok(report);
            }
        };
        report.blob = Some(record.git_blob.clone());

        self.check_sections(&record, &mut report);
        check_provenance(&record, expected, &mut report);
        self.check_manifest(&record, &mut report);
        self.check_lineage(&record, &entries, &mut report);
        self.check_scope(&head.records, &record, &entries, &mut report);
        Ok(report)
    }

    /// The research ID to check: explicit, else the worktree writer's binding.
    fn delivered_id(
        &self,
        expected: &Expected<'_>,
        report: &mut DeliveryReport,
    ) -> Result<Option<String>> {
        let binding = git::read::own_git_dir(self.root())?.join(BINDING);
        let bound = match fs::read_to_string(&binding) {
            Ok(bound) => Some(bound.trim().to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(match (expected.research_id, bound) {
            (Some(id), Some(bound)) if id != bound => {
                report.fail(
                    Reason::ResearchUnbound,
                    format!("This worktree's writer is bound to {bound}, not {id}"),
                );
                None
            }
            (Some(id), _) => Some(id.to_owned()),
            (None, Some(bound)) => Some(bound),
            (None, None) => {
                report.fail(
                    Reason::ResearchUnbound,
                    "No research record was written in this checkout with the worktree-mode writer, and no research_id was given",
                );
                None
            }
        })
    }

    /// The owner schema's README sections, present in order and written.
    fn check_sections(&self, record: &Record, report: &mut DeliveryReport) {
        let wanted: Vec<&str> = self.contract.schema["x-observatory"]["readme_sections"]
            .as_array()
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_else(|| vec!["Question", "Method", "Result", "Limitations", "Next"]);
        let sections = sections(&record.body);
        let found: Vec<&str> = sections
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| wanted.contains(name))
            .collect();
        if found != wanted {
            report.fail(
                Reason::SectionMissing,
                format!(
                    "{}: sections must be exactly `## {}` in that order; found {}",
                    record.path,
                    wanted.join("`, `## "),
                    if found.is_empty() {
                        "none".into()
                    } else {
                        found.join(", ")
                    }
                ),
            );
        }
        let placeholders: Vec<&str> = sections
            .iter()
            .filter(|(name, text)| wanted.contains(name) && is_placeholder(text))
            .map(|(name, _)| *name)
            .collect();
        if !placeholders.is_empty() {
            report.fail(
                Reason::SectionPlaceholder,
                format!(
                    "{}: `## {}` still empty or holding the reserved stub's text",
                    record.path,
                    placeholders.join("`, `## ")
                ),
            );
        }
    }

    /// `data/manifest.json` is valid and every local input matches its digest.
    fn check_manifest(&self, record: &Record, report: &mut DeliveryReport) {
        let Some(directory) = record.path.strip_suffix("/README.md") else {
            report.fail(
                Reason::ManifestInvalid,
                format!("{} is not a research directory README", record.path),
            );
            return;
        };
        let manifest_path = format!("{directory}/data/manifest.json");
        let manifest = match self
            .working_bytes(&manifest_path)
            .and_then(|bytes| Ok(serde_json::from_slice::<Value>(&bytes)?))
            .and_then(|manifest| edit::check_manifest(&self.contract, &manifest).map(|()| manifest))
        {
            Ok(manifest) => manifest,
            Err(error) => {
                report.fail(Reason::ManifestInvalid, format!("{manifest_path}: {error}"));
                return;
            }
        };
        for (index, input) in manifest["inputs"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            // Shared datasets carry their own manifest under `_data/`.
            let (Some(name), Some(declared)) = (input["name"].as_str(), input["sha256"].as_str())
            else {
                continue;
            };
            if input["shared"].is_string() {
                continue;
            }
            let data_path = format!("{directory}/data/{name}");
            let unsafe_name = Path::new(name)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)));
            if unsafe_name {
                report.fail(
                    Reason::ManifestInvalid,
                    format!("{manifest_path}: inputs[{index}] name `{name}` is not a data path"),
                );
                continue;
            }
            let bytes = match self.working_bytes(&data_path) {
                Ok(bytes) => bytes,
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    report.unverified_inputs.push(name.to_owned());
                    continue;
                }
                Err(error) => {
                    report.fail(Reason::ManifestInvalid, format!("{data_path}: {error}"));
                    continue;
                }
            };
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if !actual.eq_ignore_ascii_case(declared) {
                report.fail(
                    Reason::ArtifactDigestMismatch,
                    format!("{data_path}: sha256 is {actual}; the manifest declares {declared}"),
                );
            } else if let Some(size) = input["size"].as_u64()
                && size != bytes.len() as u64
            {
                report.fail(
                    Reason::ArtifactDigestMismatch,
                    format!(
                        "{data_path}: size is {}; the manifest declares {size}",
                        bytes.len()
                    ),
                );
            }
        }
    }

    /// Every reference the record names exists in the checkout.
    fn check_lineage(&self, record: &Record, entries: &[RecordEntry], report: &mut DeliveryReport) {
        let present: BTreeSet<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        let fields: Vec<&str> = self.contract.schema["x-observatory"]["reference_fields"]
            .as_object()
            .map(|fields| fields.keys().map(String::as_str).collect())
            .unwrap_or_else(|| vec!["derived_from", "tests"]);
        for field in fields {
            for target in record.metadata[field]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !present.contains(target) {
                    report.fail(
                        Reason::DanglingLineage,
                        format!("{} {field} references missing {target}", record.id),
                    );
                }
            }
        }
    }

    /// Nothing but the research record changed: no allocated IDs, no other
    /// record edited or removed, and the whole corpus still passes the checker.
    fn check_scope(
        &self,
        head: &[Record],
        record: &Record,
        entries: &[RecordEntry],
        report: &mut DeliveryReport,
    ) {
        let committed: BTreeMap<&str, &Record> =
            head.iter().map(|r| (r.path.as_str(), r)).collect();
        let mut clean = true;
        for entry in entries {
            if !committed.contains_key(entry.path.as_str()) {
                clean = false;
                report.fail(
                    Reason::IdAllocated,
                    format!(
                        "{} ({}) is not in the checkout's HEAD; only primary-mode create allocates IDs",
                        entry.id, entry.path
                    ),
                );
            }
        }
        let present: BTreeSet<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        for path in committed.keys().filter(|path| !present.contains(*path)) {
            clean = false;
            report.fail(
                Reason::OtherRecordChanged,
                format!("{path} was removed from the checkout"),
            );
        }
        if !clean {
            return;
        }
        let records = match self.read_records(&self.contract, &report.revision, None) {
            Ok(records) => records,
            Err(error) => {
                report.fail(Reason::CorpusInvalid, error.to_string());
                return;
            }
        };
        for current in records.values().filter(|r| r.id != record.id) {
            if committed
                .get(current.path.as_str())
                .is_some_and(|before| before.content_sha256 != current.content_sha256)
            {
                report.fail(
                    Reason::OtherRecordChanged,
                    format!(
                        "{} changed; a run writes only its research record {}",
                        current.path, record.id
                    ),
                );
            }
        }
        // The checker's whole-corpus rules are the catch-all for anything the
        // specific checks above did not already name.
        if report.findings.is_empty()
            && let Err(error) = validate_records(&records)
        {
            report.fail(Reason::CorpusInvalid, error.to_string());
        }
    }
}

impl DeliveryReport {
    fn fail(&mut self, reason: Reason, message: impl Into<String>) {
        self.findings.push(Finding {
            reason,
            message: message.into(),
        });
    }
}

fn check_provenance(record: &Record, expected: &Expected<'_>, report: &mut DeliveryReport) {
    for (field, want, reason) in [
        ("task", expected.task, Reason::OrbitTaskMismatch),
        ("run", expected.run, Reason::OrbitRunMismatch),
    ] {
        match record.metadata["orbit"][field].as_str() {
            Some(found) if found == want => (),
            Some(found) => report.fail(
                reason,
                format!(
                    "{} orbit.{field} is `{found}`; expected `{want}`",
                    record.id
                ),
            ),
            None => report.fail(
                reason,
                format!("{} has no orbit.{field}; expected `{want}`", record.id),
            ),
        }
    }
}

/// Level-2 headings (`## Name`, as the checker matches them) and their text.
fn sections(body: &str) -> Vec<(&str, String)> {
    let mut sections: Vec<(&str, String)> = Vec::new();
    for line in body.lines() {
        let heading = line
            .strip_prefix("##")
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .map(str::trim)
            .filter(|name| !name.is_empty());
        match (heading, sections.last_mut()) {
            (Some(name), _) => sections.push((name, String::new())),
            (None, Some((_, text))) => {
                text.push_str(line);
                text.push('\n');
            }
            (None, None) => (),
        }
    }
    sections
}

fn is_placeholder(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || record::RESEARCH_PLACEHOLDERS
            .iter()
            .any(|placeholder| text.eq_ignore_ascii_case(placeholder))
}
