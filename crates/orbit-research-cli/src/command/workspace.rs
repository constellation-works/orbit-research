//! Workspace command boundary.

use serde_json::Value;
use std::path::Path;

pub(crate) fn initialize(path: &Path) -> Result<Value, String> {
    orbit_research_core::init_workspace(path).map_err(|error| error.to_string())
}

pub(crate) fn prepare_operations(path: &Path) -> Result<Value, String> {
    orbit_research_core::prepare_workspace_operations(path).map_err(|error| error.to_string())
}
