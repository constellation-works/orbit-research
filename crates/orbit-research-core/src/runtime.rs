//! Process-scoped state shared by all application transports.
use crate::{Reservation, Result, Snapshot, adapter::orbit::OrbitBackend};
use orbit_research_store::corpus::Corpus;
use std::path::Path;

/// Orbit authority and publication target are fixed at startup.
pub struct Application {
    pub(crate) corpus: Research,
    pub(crate) orbit: Option<OrbitBackend>,
    pub(crate) publication_ref: String,
}

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
