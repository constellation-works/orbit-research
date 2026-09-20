//! Application composition and coordinated research operations.
pub mod adapter;
pub mod application;
pub mod bootstrap;
pub mod config;
pub mod runtime;

pub use bootstrap::init_workspace;
pub use orbit_research_common::{Error, Record, Reservation, Result, Snapshot};
pub use runtime::{Application, Research};
// Preserve existing callers while internal ownership follows the module layers.
pub use adapter::orbit as backend;
pub use application::{api, legacy_import, operations, receipt, work};

// Legacy command compatibility never receives Markdown application writes.
pub use orbit_research_common::legacy_contract;
pub use orbit_research_store::{legacy_index, legacy_owner};

/// Packaged research guidance exposed through the CLI resource command.
pub const RESEARCH_NATIVE_SKILL: &str = include_str!("../assets/skills/research-native/SKILL.md");
