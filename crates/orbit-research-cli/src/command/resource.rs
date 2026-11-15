//! The packaged research skill and its references.

use orbit_research_core::{RESEARCH_LOOP_REFERENCES, RESEARCH_LOOP_SKILL};
use serde_json::{Map, Value, json};

/// The skill with every reference, or the one named reference. Clap admits
/// only the packaged names, so an unknown name never reaches this function.
pub(crate) fn render(reference: Option<&str>) -> (Value, u8) {
    let value = match reference.and_then(|name| {
        RESEARCH_LOOP_REFERENCES
            .iter()
            .find(|(packaged, _)| *packaged == name)
    }) {
        Some((name, text)) => json!({"version": 1, "reference": {"name": name, "text": text}}),
        None => {
            let references: Map<String, Value> = RESEARCH_LOOP_REFERENCES
                .iter()
                .map(|(name, text)| ((*name).to_owned(), json!(text)))
                .collect();
            json!({"version": 1, "skill": RESEARCH_LOOP_SKILL, "references": references})
        }
    };
    (value, 0)
}
