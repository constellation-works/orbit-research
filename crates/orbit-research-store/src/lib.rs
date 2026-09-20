//! Canonical corpus and operational journal persistence. No transports or runtime.
pub mod corpus;
pub mod journal;
pub mod legacy_index;
pub mod legacy_owner;
#[cfg(test)]
mod tests;
pub mod workspace;
pub mod writer;
pub use orbit_research_common::{Error, Result};
