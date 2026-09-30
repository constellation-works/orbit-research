//! Local request correlations. Scientific records remain entirely in the corpus.
//! Rebuildable links point to Orbit, which owns task/run authority.
use crate::{Error, Research as Corpus, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A recorded intent to link a reserved research item to an Orbit task under
/// `request_key`. Persisted before the task is created (or adopted) so a
/// retry can recognize an uncertain prior outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub request_key: String,
    pub correlation_tag: String,
    pub research_id: String,
    pub task_id: Option<String>,
}

/// The result of recording (or recalling) a link intent.
pub struct LinkPreparation {
    pub link: Link,
    /// True when this call created the intent; false when an identical retry
    /// recalled one already recorded.
    pub is_new: bool,
    /// The reserved research item's directory, as `dir:` context_files.
    pub context_files: Vec<String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Corpus {
    pub fn work_links(&self) -> Result<Vec<Link>> {
        self.store.request_log()?.list()
    }

    /// Record intent to link `research_id` under `request_key`, or recall an
    /// identical retry's prior intent. Refuses when `research_id` was never
    /// reserved, or when the key was already used for a different item.
    pub fn link_intent(&self, request_key: &str, research_id: &str) -> Result<LinkPreparation> {
        if request_key.is_empty() || request_key.len() > 256 {
            return Err(Error::Invalid(
                "Request key must contain 1-256 bytes".into(),
            ));
        }
        let snapshot = self.store.committed_snapshot()?;
        let record = snapshot
            .records
            .iter()
            .find(|r| r.id == research_id && r.kind == "R")
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "{research_id} must be reserved before it can be linked"
                ))
            })?;
        let directory = std::path::Path::new(&record.path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing research directory".into()))?
            .to_string_lossy()
            .into_owned();
        let context_files = vec![format!("dir:{directory}")];

        let log = self.store.request_log()?;
        let key = digest(request_key.as_bytes());
        if let Some(existing) = log.read::<Link>(&key)? {
            if existing.research_id != research_id {
                return Err(Error::Conflict(format!(
                    "Request key was already used to link {}, not {research_id}",
                    existing.research_id
                )));
            }
            return Ok(LinkPreparation {
                link: existing,
                is_new: false,
                context_files,
            });
        }
        let link = Link {
            request_key: request_key.into(),
            correlation_tag: format!("research-request:{request_key}"),
            research_id: research_id.into(),
            task_id: None,
        };
        log.save(&key, &link)?;
        Ok(LinkPreparation {
            link,
            is_new: true,
            context_files,
        })
    }

    /// Record the Orbit task adopted or created for a previously recorded
    /// link intent. An identical retry recalls the task; a different task
    /// refuses without replacing the original correlation.
    pub fn link_confirm(&self, request_key: &str, task_id: &str) -> Result<Link> {
        let log = self.store.request_log()?;
        let key = digest(request_key.as_bytes());
        let mut link: Link = log
            .read(&key)?
            .ok_or_else(|| Error::Invalid("No recorded link intent for this request key".into()))?;
        if let Some(existing) = &link.task_id {
            if existing != task_id {
                return Err(Error::Conflict(
                    "Request key is already linked to a different task".into(),
                ));
            }
            return Ok(link);
        }
        link.task_id = Some(task_id.into());
        log.save(&key, &link)?;
        Ok(link)
    }
}
