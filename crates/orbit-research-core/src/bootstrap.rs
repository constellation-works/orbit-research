//! Assemble application state without starting an execution engine.
use crate::{Research, Result, runtime::Application};
use std::path::Path;

impl Application {
    pub fn local(root: &Path) -> Result<Self> {
        Ok(Self {
            corpus: Research::open(root)?,
        })
    }
}

pub fn init_workspace(path: &Path) -> Result<serde_json::Value> {
    orbit_research_store::workspace::init(path)
}
