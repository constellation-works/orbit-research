//! Application composition and coordinated research operations.
pub mod adapter;
pub mod application;
pub mod bootstrap;
pub mod config;
pub mod runtime;

pub use bootstrap::init_workspace;
pub use config::{BackendConfig, BackendSettings};
pub use orbit_research_common::{Error, Record, Reservation, Result, Snapshot};
pub use runtime::{Application, Research};
// Preserve existing callers while internal ownership follows the module layers.
pub use adapter::orbit as backend;
pub use application::{api, operations, receipt, work};

/// Packaged research guidance exposed through the CLI resource command.
pub const RESEARCH_NATIVE_SKILL: &str = include_str!("../assets/skills/research-native/SKILL.md");
