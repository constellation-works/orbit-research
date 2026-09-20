//! Canonical corpus and request-log persistence. No transports or runtime.
pub mod corpus;
pub mod request_log;
#[cfg(test)]
mod tests;
pub mod workspace;
pub mod writer;
pub use orbit_research_common::{Error, Result};
