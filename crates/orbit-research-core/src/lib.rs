//! Application composition and coordinated research operations.
pub mod application;
pub mod bootstrap;
pub mod runtime;

pub use bootstrap::{init_workspace, prepare_workspace_operations};
pub use orbit_research_common::{
    AcceptanceFailure, Error, Record, Reservation, Result, Snapshot, render_issues_with,
};
pub use orbit_research_store::delivery;
pub use runtime::{Application, Research};
// Preserve existing callers while internal ownership follows the module layers.
pub use application::{api, operations, work};

/// Packaged research guidance exposed through the CLI resource command. The
/// plugin skill is the single copy: Orbit installs it from `.orbit-plugin/`
/// and the binary embeds the same file.
pub const RESEARCH_NATIVE_SKILL: &str =
    include_str!("../../../.orbit-plugin/skills/native/SKILL.md");

/// This crate's own version, reported by the plugin `version` tool. Corpus-independent:
/// callers needing a health signal when the corpus itself cannot open still get an answer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
