//! The writer's production acceptance lookup. `assess` runs outside the plugin
//! sandbox, so it may spawn `orbit`: it fetches the `research-acceptance.json`
//! artifact the plugin's `accept` tool stored on the record's task. Core owns
//! what the evidence must say; this adapter only fetches and decodes it, and
//! turns every way the fetch can fail into a typed refusal, never an accept.
use crate::plugin::{ACCEPTANCE_ARTIFACT_PATH, orbit_binary, read_task_artifact};
use orbit_research_core::{
    AcceptanceFailure, Result,
    application::acceptance::{Acceptance, AcceptanceLookup},
};
use std::path::PathBuf;

/// Reads one named artifact of one Orbit task as text, `None` when the task
/// never stored it. The seam that lets tests substitute Orbit.
pub(crate) trait ArtifactReader {
    fn read(&self, task: &str, path: &str) -> Result<Option<String>>;
}

/// Spawns `orbit tool run` from the corpus checkout, which is the Orbit
/// workspace; `orbit` resolves as the plugin transport does (`ORBIT_BIN`, else
/// `PATH`).
pub(crate) struct OrbitArtifacts {
    orbit: String,
    workspace: PathBuf,
}

impl OrbitArtifacts {
    pub(crate) fn new(orbit: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            orbit: orbit.into(),
            workspace: workspace.into(),
        }
    }
}

impl ArtifactReader for OrbitArtifacts {
    fn read(&self, task: &str, path: &str) -> Result<Option<String>> {
        read_task_artifact(&self.orbit, Some(&self.workspace), task, path)
    }
}

pub(crate) struct OrbitAcceptance<R = OrbitArtifacts> {
    artifacts: R,
}

impl OrbitAcceptance {
    /// The production lookup for the corpus checked out at `corpus`.
    pub(crate) fn for_corpus(corpus: impl Into<PathBuf>) -> Self {
        Self::new(OrbitArtifacts::new(orbit_binary(), corpus))
    }
}

impl<R: ArtifactReader> OrbitAcceptance<R> {
    pub(crate) fn new(artifacts: R) -> Self {
        Self { artifacts }
    }
}

impl<R: ArtifactReader> AcceptanceLookup for OrbitAcceptance<R> {
    fn acceptance(&self, research_id: &str, task: &str) -> Result<Option<Acceptance>> {
        let text = self
            .artifacts
            .read(task, ACCEPTANCE_ARTIFACT_PATH)
            .map_err(|error| AcceptanceFailure::Unreachable {
                research: research_id.into(),
                task: task.into(),
                reason: error.to_string(),
            })?;
        let Some(text) = text else {
            return Ok(None);
        };
        serde_json::from_str(&text).map(Some).map_err(|error| {
            AcceptanceFailure::Malformed {
                research: research_id.into(),
                task: task.into(),
                reason: error.to_string(),
            }
            .into()
        })
    }
}
