//! Local request correlations. Scientific records remain entirely in the corpus.
//! Rebuildable links point to Orbit, which owns task/run authority.
use super::work::{find_research, resolve_link_scope};
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
    /// The task scope this request key was recorded with. Empty only in a link
    /// recorded before scopes were persisted, which always used the research
    /// directory.
    #[serde(default)]
    pub context_files: Vec<String>,
}

/// The result of recording (or recalling) a link intent.
#[derive(Debug)]
pub struct LinkPreparation {
    pub link: Link,
    /// True when this call created the intent; false when an identical retry
    /// recalled one already recorded.
    pub is_new: bool,
    /// The scope the request key is bound to: the one just recorded, or the
    /// one a recalled retry matched.
    pub context_files: Vec<String>,
}

/// A link's request key is 1-256 bytes. An input error, not a corpus problem.
pub fn check_request_key(request_key: &str) -> Result<()> {
    if request_key.is_empty() || request_key.len() > 256 {
        return Err(Error::InvalidInput(
            "The request key must be 1-256 bytes long; use a short, non-empty identifier of your own and reuse it only to retry the same link".into(),
        ));
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Corpus {
    pub fn work_links(&self) -> Result<Vec<Link>> {
        self.store.request_log()?.list()
    }

    /// The task scope a link to `research_id` carries. With no `supplied`
    /// value it is the reserved research item's directory; a supplied value is
    /// accepted only when it equals what `plan` derives for that item (the
    /// directory, one contribution unit's `code/` and `artifacts/` paths, or the
    /// synthesis files), and refused otherwise. Reads only the committed
    /// snapshot; records nothing.
    fn link_scope(&self, research_id: &str, supplied: Option<&[String]>) -> Result<Vec<String>> {
        let snapshot = self.store.committed_snapshot()?;
        let record = find_research(&snapshot.records, research_id, "linking")?;
        resolve_link_scope(record, research_id, supplied)
    }

    /// Record intent to link `research_id` under `request_key`, or recall an
    /// identical retry's prior intent. `context_files` is the task scope the
    /// caller asks for (see `link_scope`); `None` means the research directory.
    /// Refuses when `research_id` was never reserved, when the scope is not one
    /// `plan` derives, when the key was already used for a different item, or
    /// when it was recorded with a different scope.
    pub fn link_intent(
        &self,
        request_key: &str,
        research_id: &str,
        context_files: Option<&[String]>,
    ) -> Result<LinkPreparation> {
        check_request_key(request_key)?;
        let context_files = self.link_scope(research_id, context_files)?;

        let log = self.store.request_log()?;
        let key = digest(request_key.as_bytes());
        if let Some(existing) = log.read::<Link>(&key)? {
            if existing.research_id != research_id {
                return Err(Error::Conflict(format!(
                    "This request key was already used to link {}, not {research_id}; choose a new request key for {research_id} (reuse a key only to retry the same link)",
                    existing.research_id
                )));
            }
            let recorded = if existing.context_files.is_empty() {
                self.link_scope(research_id, None)?
            } else {
                existing.context_files.clone()
            };
            if recorded != context_files {
                return Err(Error::Conflict(format!(
                    "This request key was already used to link {research_id} with the task scope {recorded:?}, not {context_files:?}; choose a new request key for the new scope (reuse a key only to retry the same link)"
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
            context_files: context_files.clone(),
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
