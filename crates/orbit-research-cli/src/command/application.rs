//! Process-scoped application composition.

use std::path::Path;

use crate::acceptance::OrbitAcceptance;
use crate::output::Invalid;

/// Open the corpus and wire the production acceptance lookup, which reads the
/// plugin's `research-acceptance.json` task artifact through Orbit. Every
/// transport composes here, so `assess` verifies acceptance the same way
/// through the CLI and MCP.
pub(crate) fn compose(corpus: &Path) -> Result<orbit_research_core::api::Application, Invalid> {
    orbit_research_core::api::Application::local(corpus)
        .map(|application| application.with_acceptance(OrbitAcceptance::for_corpus(corpus)))
        .map_err(|error| error.to_string().into())
}
