//! Acceptance lookup consulted before an assessment is appended. Acceptance
//! evidence is operational state owned by Orbit: the plugin's `accept` tool
//! persists it as the `research-acceptance.json` task artifact, in exactly
//! this shape, so a lookup backed by that artifact can deserialize it
//! directly. `Application::local`'s default lookup finds nothing until a
//! caller wires a real one with `with_acceptance`, so `assess` refuses; the
//! CLI wires one that reads the artifact through Orbit. Core never spawns a
//! process: the lookup is the seam, and Core owns what the evidence must say.
use crate::{Result, Snapshot};
use orbit_research_common::AcceptanceFailure;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Evidence that a delivered research result was validated and accepted.
/// Mirrors `research-acceptance.json`, the task artifact the plugin's
/// `accept` tool persists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub research_id: String,
    /// Published commit the result was validated at.
    pub commit: String,
    /// Git blob of the accepted research README at that commit.
    pub blob: String,
    pub run_id: String,
    /// sha256 of each locally-verified manifest input, by name.
    #[serde(default)]
    pub artifact_digests: BTreeMap<String, String>,
}

impl Acceptance {
    /// Whether this evidence accepts `research`'s README as it is now
    /// (`blob`). The one bar for "accepted": `assess` applies it before
    /// appending a verdict and the dashboard panel applies it to decide a row
    /// is done, so the two cannot disagree. `task` is only named in the refusal.
    pub fn verify(
        &self,
        research: &str,
        task: &str,
        blob: &str,
    ) -> std::result::Result<(), AcceptanceFailure> {
        if self.research_id != research {
            return Err(AcceptanceFailure::WrongResearch {
                research: research.into(),
                task: task.into(),
                found: self.research_id.clone(),
            });
        }
        if self.blob != blob {
            return Err(AcceptanceFailure::StaleBlob {
                research: research.into(),
                task: task.into(),
                accepted: self.blob.clone(),
                current: blob.into(),
            });
        }
        Ok(())
    }
}

pub trait AcceptanceLookup {
    /// The acceptance stored on `task` for `research_id`, if any. `task` is
    /// the record's own `orbit.task`. An implementation reports an unreachable
    /// store or an unreadable artifact as an error (`AcceptanceFailure::
    /// Unreachable` or `Malformed`) and never as an accepted result; it does
    /// not judge whether the evidence matches the record, which Core does.
    fn acceptance(&self, research_id: &str, task: &str) -> Result<Option<Acceptance>>;
}

/// The default lookup: no acceptance storage is available.
pub struct NoAcceptanceStore;

impl AcceptanceLookup for NoAcceptanceStore {
    fn acceptance(&self, _: &str, _: &str) -> Result<Option<Acceptance>> {
        Ok(None)
    }
}

/// Refuse unless `research` is a delivered R whose current README is exactly
/// what its task accepted. Runs inside the writer's lock on a clean checkout,
/// so the snapshot's README blob is the one at HEAD.
pub(crate) fn require_acceptance(
    lookup: &dyn AcceptanceLookup,
    snapshot: &Snapshot,
    research: &str,
) -> Result<()> {
    let record = snapshot
        .records
        .iter()
        .find(|record| record.id == research && record.kind == "R")
        .ok_or_else(|| crate::Error::NotFound(format!("Unknown research record id: {research}")))?;
    let task = record.metadata["orbit"]["task"]
        .as_str()
        .filter(|task| !task.trim().is_empty())
        .ok_or_else(|| AcceptanceFailure::NoTask {
            research: research.into(),
        })?;
    let acceptance =
        lookup
            .acceptance(research, task)?
            .ok_or_else(|| AcceptanceFailure::Missing {
                research: research.into(),
                task: task.into(),
            })?;
    acceptance.verify(research, task, &record.git_blob)?;
    Ok(())
}
