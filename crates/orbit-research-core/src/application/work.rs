//! Drafts Orbit tasks without creating another task engine or scheduling state.
use crate::{Error, Research as Corpus, Result};
use orbit_research_common::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A drafted Orbit task, shaped for `orbit.task.add`: the caller (or the
/// plugin's `link` tool) supplies these fields verbatim.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskDraft {
    #[schemars(length(min = 1))]
    pub title: String,
    #[schemars(length(min = 1))]
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub context_files: Vec<String>,
}

/// Selects what `plan` drafts. Each shape carries only the fields it needs.
#[derive(Debug, Clone)]
pub enum PlanShape {
    /// One task owns the complete investigation, including its result summary.
    Investigation { objective: String },
    /// Parallel contributors own only their own code/artifact paths.
    Contribution { unit: String, objective: String },
    /// Reconciles completed contributions into the shared summary.
    Synthesis { units: Vec<String> },
}

fn find_research<'a>(records: &'a [Record], research_id: &str) -> Result<&'a Record> {
    records
        .iter()
        .find(|r| r.id == research_id && r.kind == "R")
        .ok_or_else(|| Error::Invalid("Research item must be reserved before planning work".into()))
}

fn research_directory(record: &Record) -> Result<String> {
    Ok(std::path::Path::new(&record.path)
        .parent()
        .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
        .to_string_lossy()
        .into_owned())
}

fn checked_unit(unit: &str) -> Result<()> {
    if unit.is_empty()
        || unit.len() > 80
        || unit.starts_with('-')
        || unit.ends_with('-')
        || unit.contains("--")
        || !unit
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::Invalid(
            "Work unit must be a lowercase kebab-case name of at most 80 characters".into(),
        ));
    }
    Ok(())
}

impl Corpus {
    /// Draft an Orbit task for `research_id` in the given shape. Read-only:
    /// creates nothing, in the corpus or in Orbit.
    pub fn plan(&self, research_id: &str, shape: PlanShape) -> Result<TaskDraft> {
        let snapshot = self.store.committed_snapshot()?;
        let record = find_research(&snapshot.records, research_id)?;
        let directory = research_directory(record)?;
        match shape {
            PlanShape::Investigation { objective } => {
                if objective.trim().is_empty() {
                    return Err(Error::Invalid("Work objective is required".into()));
                }
                Ok(TaskDraft {
                    title: format!("Investigate {research_id}"),
                    description: format!(
                        "{objective}\n\nWork only within {directory}/. Preserve scripts/notebooks in code/ and output evidence in artifacts/. Bind the result to the executing Orbit task/run. Preserve failed controls and uncertainty; execution success is not scientific support. Do not edit other records or hypothesis assessments."
                    ),
                    acceptance_criteria: vec![
                        format!(
                            "{path} documents Question, Method, Result, Limitations and Next",
                            path = record.path
                        ),
                        format!("{directory}/data/manifest.json is reconciled with the delivered artifacts"),
                        "The published result is bound to the executing Orbit task/run and passes validate".into(),
                    ],
                    context_files: vec![format!("dir:{directory}")],
                })
            }
            PlanShape::Contribution { unit, objective } => {
                checked_unit(&unit)?;
                if objective.trim().is_empty() {
                    return Err(Error::Invalid("Work objective is required".into()));
                }
                let code = format!("{directory}/code/{unit}");
                let artifacts = format!("{directory}/artifacts/{unit}");
                Ok(TaskDraft {
                    title: format!("Contribute {unit} to {research_id}"),
                    description: format!(
                        "{objective}\n\nWork within {code}/ and {artifacts}/ only. Read {path} for context; do not edit it or the shared data/manifest.json. Preserve scripts and notebooks as source artifacts; the app treats their contents as opaque.",
                        path = record.path
                    ),
                    acceptance_criteria: vec![
                        format!(
                            "{artifacts}/findings.md documents Question, Method, Result, Limitations and Next for {unit}"
                        ),
                        format!("{artifacts}/manifest.json records input/output digests"),
                        format!(
                            "No edits outside {code}/ and {artifacts}/, and no change to hypothesis assessments"
                        ),
                    ],
                    context_files: vec![format!("dir:{code}"), format!("dir:{artifacts}")],
                })
            }
            PlanShape::Synthesis { units } => {
                if units.is_empty() {
                    return Err(Error::Invalid(
                        "Synthesis needs at least one contributing work unit".into(),
                    ));
                }
                let mut inputs = Vec::new();
                let mut unique = std::collections::BTreeSet::new();
                for unit in &units {
                    if !unique.insert(unit) {
                        return Err(Error::Invalid("Duplicate synthesis work unit".into()));
                    }
                    checked_unit(unit)?;
                    inputs.push(format!("{directory}/artifacts/{unit}/findings.md"));
                }
                let manifest = format!("{directory}/data/manifest.json");
                Ok(TaskDraft {
                    title: format!("Synthesize {research_id} from {}", units.join(", ")),
                    description: format!(
                        "Synthesize the completed contributions from {} into {}. Preserve conflicting evidence, failed controls and limitations; execution success is not scientific support. Reconcile data/manifest.json and retain all contribution source and artifacts. Do not rewrite H/T assessments without a separate explicit assessment task.",
                        inputs.join(", "),
                        record.path
                    ),
                    acceptance_criteria: vec![
                        format!(
                            "{path} reconciles all listed contributions into the canonical Question, Method, Result, Limitations and Next",
                            path = record.path
                        ),
                        format!("{manifest} is reconciled and retains all contribution source and artifacts"),
                        "Publication evidence is bound to the final commit, record blob and artifact digests".into(),
                    ],
                    context_files: vec![format!("file:{}", record.path), format!("file:{manifest}")],
                })
            }
        }
    }
}
