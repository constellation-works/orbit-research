//! Acceptance lookup consulted before an assessment is appended. Acceptance
//! evidence is operational state owned by Orbit: the plugin's `accept` tool
//! persists it as the `research-acceptance.json` task artifact, in exactly
//! this shape, so a lookup backed by that artifact can deserialize it
//! directly. `Application::local`'s default lookup finds nothing until a
//! caller wires a real one with `with_acceptance`, so `assess` refuses.
use crate::Result;
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

pub trait AcceptanceLookup {
    /// The stored acceptance for a research record, if any.
    fn acceptance(&self, research_id: &str) -> Result<Option<Acceptance>>;
}

/// The default lookup: no acceptance storage is available yet.
pub struct NoAcceptanceStore;

impl AcceptanceLookup for NoAcceptanceStore {
    fn acceptance(&self, _: &str) -> Result<Option<Acceptance>> {
        Ok(None)
    }
}
