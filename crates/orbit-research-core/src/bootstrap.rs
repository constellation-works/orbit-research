//! Assemble application state without starting an execution engine.
use crate::{Research, Result, application::acceptance::NoAcceptanceStore, runtime::Application};
use std::path::Path;

impl Application {
    pub fn local(root: &Path) -> Result<Self> {
        Ok(Self {
            corpus: Research::open(root)?,
            acceptance: Box::new(NoAcceptanceStore),
        })
    }
}

pub fn init_workspace(path: &Path) -> Result<serde_json::Value> {
    orbit_research_store::workspace::init(path)
}

/// Explicitly prepare shared operational state on an existing primary corpus.
pub fn prepare_workspace_operations(path: &Path) -> Result<serde_json::Value> {
    orbit_research_store::request_log::prepare_workspace_operations(path)
}
