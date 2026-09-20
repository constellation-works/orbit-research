//! Application composition and coordinated research operations.
//! Lower storage is private; CLI, Web and MCP delegate through this boundary.
pub mod api;
pub mod backend;
pub mod legacy_import;
pub mod operations;
pub mod receipt;
pub mod work;
pub use orbit_research_common::{Error, Record, Reservation, Result, Snapshot};
use orbit_research_store::corpus::Corpus;
use std::path::Path;

pub struct Research {
    pub(crate) store: Corpus,
}
impl Research {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            store: Corpus::open(root)?,
        })
    }
    pub fn root(&self) -> &Path {
        self.store.root()
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.store.snapshot()
    }
    pub fn reserve(
        &self,
        key: &str,
        kind: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
        parents: Vec<String>,
    ) -> Result<Reservation> {
        self.store.reserve(key, kind, title, body, tags, parents)
    }
    pub fn revise_question(
        &self,
        id: &str,
        blob: &str,
        title: &str,
        body: &str,
        tags: Vec<String>,
    ) -> Result<Reservation> {
        self.store.revise_question(id, blob, title, body, tags)
    }
}
pub fn init_workspace(path: &Path) -> Result<serde_json::Value> {
    orbit_research_store::workspace::init(path)
}

// Retained command compatibility; the Markdown application never dual-writes these formats.
pub use orbit_research_common::legacy_contract;
pub use orbit_research_store::{legacy_index, legacy_owner};
