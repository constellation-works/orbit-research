//! Local request correlations. Scientific records remain entirely in the corpus.
//! Rebuildable links point to Orbit, which owns task/run authority.
use crate::{Research as Corpus, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub request_digest: String,
    pub correlation_tag: String,
    pub workspace: String,
    pub owner_machine_id: String,
    pub research_id: String,
    pub task_id: Option<String>,
}

impl Corpus {
    pub fn work_links(&self) -> Result<Vec<Link>> {
        self.store.request_log()?.list()
    }
}
