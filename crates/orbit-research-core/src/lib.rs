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
/// and the binary embeds the same files.
pub const RESEARCH_LOOP_SKILL: &str = include_str!("../../../.orbit-plugin/skills/loop/SKILL.md");

/// The skill's `references/<name>.md` files, by name, in the order the skill
/// lists them. The CLI's skill tests fail when this list and the directory differ.
pub const RESEARCH_LOOP_REFERENCES: &[(&str, &str)] = &[
    (
        "own-session",
        include_str!("../../../.orbit-plugin/skills/loop/references/own-session.md"),
    ),
    (
        "corpus",
        include_str!("../../../.orbit-plugin/skills/loop/references/corpus.md"),
    ),
    (
        "link",
        include_str!("../../../.orbit-plugin/skills/loop/references/link.md"),
    ),
    (
        "operator-mcp",
        include_str!("../../../.orbit-plugin/skills/loop/references/operator-mcp.md"),
    ),
    (
        "validate",
        include_str!("../../../.orbit-plugin/skills/loop/references/validate.md"),
    ),
    (
        "accept-assess",
        include_str!("../../../.orbit-plugin/skills/loop/references/accept-assess.md"),
    ),
    (
        "panels",
        include_str!("../../../.orbit-plugin/skills/loop/references/panels.md"),
    ),
];

/// This crate's own version, reported by the plugin `version` tool. Corpus-independent:
/// callers needing a health signal when the corpus itself cannot open still get an answer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
