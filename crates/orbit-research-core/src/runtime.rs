//! Process-scoped state shared by all application transports.
use crate::{Reservation, Result, Snapshot, application::acceptance::AcceptanceLookup};
use orbit_research_store::corpus::Corpus;
use std::path::Path;

pub struct Application {
    pub(crate) corpus: Research,
    pub(crate) acceptance: Box<dyn AcceptanceLookup>,
}

impl Application {
    /// Replace the acceptance lookup `assess` consults.
    pub fn with_acceptance(mut self, lookup: impl AcceptanceLookup + 'static) -> Self {
        self.acceptance = Box::new(lookup);
        self
    }
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
