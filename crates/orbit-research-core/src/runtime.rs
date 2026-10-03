//! Process-scoped state shared by all application transports.
use crate::{Reservation, Result, Snapshot, application::acceptance::AcceptanceLookup};
use orbit_research_store::{
    corpus::Corpus,
    delivery::{DeliveryReport, Expected},
};
use std::path::Path;

pub struct Application {
    pub(crate) corpus: Research,
    pub(crate) acceptance: Box<dyn AcceptanceLookup>,
}

impl Application {
    /// The sandboxed plugin must use state prepared outside Git metadata.
    pub fn require_prepared_operations(&self) -> Result<()> {
        self.corpus.store.require_prepared_operations()
    }

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

    /// Records and owner schema read from the checkout's HEAD commit, ignoring
    /// uncommitted working-tree edits. Work plans and links read this view.
    pub fn committed_snapshot(&self) -> Result<Snapshot> {
        self.store.committed_snapshot()
    }

    /// The delivery gate: check one research record in this checkout (a run
    /// worktree or the primary) against the checkout's HEAD. Read-only.
    pub fn validate_delivery(&self, expected: &Expected<'_>) -> Result<DeliveryReport> {
        self.store.validate_delivery(expected)
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
