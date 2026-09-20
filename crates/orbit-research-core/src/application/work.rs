//! Plans Orbit work without creating another task engine or scheduling state.
use crate::{Error, Research as Corpus, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkPlan {
    #[schemars(length(min = 1))]
    pub research_id: String,
    #[schemars(length(min = 1))]
    pub corpus_revision: String,
    #[schemars(length(min = 1))]
    pub research_blob: String,
    pub mode: WorkMode,
    pub context_files: Vec<String>,
    #[schemars(length(min = 1))]
    pub instructions: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Investigation,
    Contribution,
    Synthesis,
}

impl Corpus {
    /// One task owns the complete investigation, including its result summary.
    /// Parallel contributions use the narrower contribution/synthesis modes.
    pub fn investigation(&self, research_id: &str, objective: &str) -> Result<WorkPlan> {
        if objective.trim().is_empty() {
            return Err(Error::Invalid("Work objective is required".into()));
        }
        let snapshot = self.store.committed_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == research_id && r.kind == "R")
            .ok_or_else(|| {
                Error::Invalid("Research item must be reserved before planning work".into())
            })?;
        let directory = std::path::Path::new(&record.path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
            .to_string_lossy();
        Ok(WorkPlan {
            research_id: research_id.into(),
            corpus_revision: snapshot.revision,
            research_blob: record.git_blob.clone(),
            mode: WorkMode::Investigation,
            context_files: vec![format!("dir:{directory}")],
            instructions: format!(
                "{objective}\n\nWork only within {directory}/. Preserve scripts/notebooks in code/ and output evidence in artifacts/. Update {path} with Question, Method, Result, Limitations and Next and reconcile data/manifest.json. Bind the result to the executing Orbit task/run. Preserve failed controls and uncertainty; execution success is not scientific support. Do not edit other records or hypothesis assessments. Publish the final commit, record blob and artifact digests for receipt validation; do not claim acceptance before that gate passes.",
                path = record.path
            ),
        })
    }

    /// A deserialized plan is untrusted. Validate its declared write scope again
    /// immediately before linking it to an executable task.
    pub(crate) fn validate_work_plan(&self, plan: &WorkPlan) -> Result<()> {
        let snapshot = self.store.committed_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == plan.research_id && r.kind == "R")
            .ok_or_else(|| Error::Invalid("Planned research record disappeared".into()))?;
        if snapshot.revision != plan.corpus_revision || record.git_blob != plan.research_blob {
            return Err(Error::Conflict(
                "Research plan is stale; refresh it before creating work".into(),
            ));
        }
        if plan.instructions.trim().is_empty() {
            return Err(Error::Invalid("Work instructions are required".into()));
        }
        let directory = std::path::Path::new(&record.path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
            .to_string_lossy();
        let valid = match plan.mode {
            WorkMode::Investigation => plan.context_files == [format!("dir:{directory}")],
            WorkMode::Synthesis => {
                plan.context_files
                    == [
                        format!("file:{}", record.path),
                        format!("file:{directory}/data/manifest.json"),
                    ]
            }
            WorkMode::Contribution => {
                let prefix = format!("dir:{directory}/code/");
                let unit = plan
                    .context_files
                    .first()
                    .and_then(|v| v.strip_prefix(&prefix));
                match unit {
                    Some(unit) => {
                        self.contribution(&plan.research_id, unit, "Validate scope")?
                            .context_files
                            == plan.context_files
                    }
                    None => false,
                }
            }
        };
        if !valid {
            return Err(Error::Invalid(
                "Work plan write scope does not match its research item and mode".into(),
            ));
        }
        Ok(())
    }

    /// Parallel contributors own only these paths. Their outputs are findings,
    /// not a change to the parent research item's scientific conclusions.
    pub fn contribution(&self, research_id: &str, unit: &str, objective: &str) -> Result<WorkPlan> {
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
        if objective.trim().is_empty() {
            return Err(Error::Invalid("Work objective is required".into()));
        }
        let snapshot = self.store.committed_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == research_id && r.kind == "R")
            .ok_or_else(|| {
                Error::Invalid("Research item must be reserved before planning work".into())
            })?;
        let directory = std::path::Path::new(&record.path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
            .to_string_lossy();
        let code = format!("{directory}/code/{unit}");
        let artifacts = format!("{directory}/artifacts/{unit}");
        Ok(WorkPlan {
            research_id: research_id.into(),
            corpus_revision: snapshot.revision,
            research_blob: record.git_blob.clone(),
            mode: WorkMode::Contribution,
            context_files: vec![format!("dir:{code}"), format!("dir:{artifacts}")],
            instructions: format!(
                "{objective}\n\nWork within {code}/ and {artifacts}/ only. Read {path} for context; do not edit it or the shared data/manifest.json. Put the contribution's Question, Method, Result, Limitations and Next in {artifacts}/findings.md, with input/output digests in {artifacts}/manifest.json. Preserve scripts and notebooks as source artifacts; the app treats their contents as opaque. Do not change hypothesis assessments or mark the parent research item complete. A subsequent synthesis task reconciles contributions into its canonical summary. Orbit owns execution, retries and file reservations.",
                path = record.path
            ),
        })
    }

    /// Synthesis is deliberately distinct from contribution; Orbit's ordinary
    /// file reservations serialize these shared writes without serializing contributors.
    pub fn synthesis(&self, research_id: &str, units: &[String]) -> Result<WorkPlan> {
        if units.is_empty() {
            return Err(Error::Invalid(
                "Synthesis needs at least one contributing work unit".into(),
            ));
        }
        let snapshot = self.store.committed_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == research_id && r.kind == "R")
            .ok_or_else(|| Error::Invalid("Unknown research item".into()))?;
        let directory = std::path::Path::new(&record.path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
            .to_string_lossy();
        let mut inputs = Vec::new();
        let mut unique = std::collections::BTreeSet::new();
        for unit in units {
            if !unique.insert(unit) {
                return Err(Error::Invalid("Duplicate synthesis work unit".into()));
            }
            self.contribution(research_id, unit, "Validate work-unit name")?;
            inputs.push(format!("{directory}/artifacts/{unit}/findings.md"));
        }
        Ok(WorkPlan {
            research_id: research_id.into(),
            corpus_revision: snapshot.revision,
            research_blob: record.git_blob.clone(),
            mode: WorkMode::Synthesis,
            context_files: vec![
                format!("file:{}", record.path),
                format!("file:{directory}/data/manifest.json"),
            ],
            instructions: format!(
                "Synthesize the completed contributions from {} into {}. Preserve conflicting evidence, failed controls and limitations; execution success is not scientific support. Reconcile data/manifest.json and retain all contribution source and artifacts. Do not rewrite H/T assessments without a separate explicit assessment task. Bind publication evidence to the final commit, record blob and artifact digests. Do not claim completion until Orbit delivery evidence and the receipt gate validate the exact published result.",
                inputs.join(", "),
                record.path
            ),
        })
    }
}
