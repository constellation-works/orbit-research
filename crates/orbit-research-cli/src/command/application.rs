//! Process-scoped application composition.

use std::path::Path;

use crate::output::Invalid;

pub(crate) fn compose(corpus: &Path) -> Result<orbit_research_core::api::Application, Invalid> {
    orbit_research_core::api::Application::local(corpus).map_err(|error| error.to_string().into())
}
