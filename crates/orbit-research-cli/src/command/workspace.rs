//! Workspace command boundary.

use crate::output::Invalid;
use serde_json::Value;
use std::path::Path;

pub(crate) fn initialize(path: &Path) -> Result<Value, Invalid> {
    orbit_research_core::init_workspace(path).map_err(|error| Invalid::from(&error))
}

pub(crate) fn prepare_operations(path: &Path) -> Result<Value, Invalid> {
    orbit_research_core::prepare_workspace_operations(path).map_err(|error| Invalid::from(&error))
}
