//! Packaged guidance for the current Markdown research workflow.

use serde_json::{Value, json};

pub(crate) fn render() -> (Value, u8) {
    (
        json!({"version":1,"skill":orbit_research_core::RESEARCH_NATIVE_SKILL}),
        0,
    )
}
