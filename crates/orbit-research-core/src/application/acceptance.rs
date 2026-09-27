//! Acceptance lookup consulted before an assessment is appended. Acceptance
//! evidence is operational state owned by Orbit: the plugin's `accept` tool
//! stores it as a task artifact. Until that storage is wired, nothing counts
//! as accepted, so `assess` refuses.
use crate::Result;
use serde::{Deserialize, Serialize};

/// Evidence that a delivered research result was validated and accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub research_id: String,
    /// Published commit the result was validated at.
    pub commit: String,
    /// Git blob of the accepted research README at that commit.
    pub blob: String,
    pub run_id: String,
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
