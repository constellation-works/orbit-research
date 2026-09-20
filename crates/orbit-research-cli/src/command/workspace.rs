//! Workspace command boundary.

use serde_json::Value;
use std::path::Path;

pub(crate) fn initialize(path: &Path) -> Result<Value, String> {
    orbit_research_core::init_workspace(path).map_err(|error| error.to_string())
}
