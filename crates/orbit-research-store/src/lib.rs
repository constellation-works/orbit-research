//! Canonical corpus and request-log persistence. No transports or runtime.
pub mod corpus;
pub mod delivery;
pub mod edit;
pub mod request_log;
mod request_log_layout;
#[cfg(test)]
mod tests;
pub mod workspace;
pub mod worktree;
pub mod writer;
pub use orbit_research_common::{Error, Result};

mod git;
mod record;
pub use record::initial_status;
mod validation;
