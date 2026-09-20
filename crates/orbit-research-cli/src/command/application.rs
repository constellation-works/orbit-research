//! Process-scoped application composition.

use std::fs;
use std::path::Path;

use crate::output::Invalid;

pub(crate) fn compose(
    corpus: &Path,
    backend_config: Option<&Path>,
) -> Result<orbit_research_core::api::Application, Invalid> {
    match backend_config {
        Some(path) => {
            let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
            let settings =
                serde_json::from_slice::<orbit_research_core::api::BackendSettings>(&bytes)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            orbit_research_core::api::Application::configured(corpus, settings)
                .map_err(|error| error.to_string().into())
        }
        None => orbit_research_core::api::Application::local(corpus)
            .map_err(|error| error.to_string().into()),
    }
}
