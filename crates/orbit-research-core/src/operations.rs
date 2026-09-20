//! Operational request journal; scientific records remain entirely in the corpus.
//! Rebuildable links point to Orbit, which owns task/run/receipt authority.
use crate::{Error, Research as Corpus, Result, backend::Backend, work::WorkPlan};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub request_digest: String,
    pub correlation_tag: String,
    pub workspace: String,
    pub owner_machine_id: String,
    pub research_id: String,
    pub task_id: Option<String>,
    pub dispatch_attempted: bool,
    pub run_id: Option<String>,
}
impl Corpus {
    pub fn link_work(
        &self,
        backend: &Backend,
        request_key: &str,
        title: &str,
        crew: &str,
        plan: &WorkPlan,
    ) -> Result<Link> {
        if request_key.is_empty() || request_key.len() > 256 {
            return Err(Error::Invalid("Invalid task request key".into()));
        }
        self.validate_work_plan(plan)?;
        // The executor workspace must own this exact corpus, not the app source repository.
        backend.inspect()?;
        if backend.config().checkout.canonicalize()? != self.root() {
            return Err(Error::Invalid(
                "Research execution workspace must own the selected corpus".into(),
            ));
        }
        let journal = self.store.operation_journal()?;
        let key = hash(request_key.as_bytes());
        let request_digest = hash(&serde_json::to_vec(&json!([
            backend.config(),
            title,
            crew,
            plan
        ]))?);
        let mut link = if let Some(mut link) = journal.read::<Link>(&key)? {
            if link.request_digest != request_digest {
                return Err(Error::Invalid(
                    "Task request key reused for different work".into(),
                ));
            }
            if link.task_id.is_some() {
                return Ok(link);
            }
            let found = backend.correlated_tasks(&link.correlation_tag)?;
            if found.len() == 1 {
                link.task_id = Some(
                    found[0]["id"]
                        .as_str()
                        .ok_or_else(|| Error::Invalid("Reconciled task has no ID".into()))?
                        .into(),
                );
                journal.save(&key, &link)?;
                return Ok(link);
            }
            return Err(Error::Invalid(format!(
                "Prior task submission is unresolved ({} matches). Do not retry creation; reconcile Orbit evidence first.",
                found.len()
            )));
        } else {
            Link {
                request_digest,
                correlation_tag: format!("research-request:{key}"),
                workspace: backend.config().workspace.clone(),
                owner_machine_id: backend.config().owner_machine_id.clone(),
                research_id: plan.research_id.clone(),
                task_id: None,
                dispatch_attempted: false,
                run_id: None,
            }
        };
        // Persist intent before the first potentially uncertain mutation.
        journal.save(&key, &link)?;
        let description = format!(
            "{}\n\nResearch item: {}\nReserved corpus revision: {}\nRecord blob: {}\nRequest correlation: {}\n\nBegin from a revision containing the committed research stub. Do not allocate IDs inside the execution worktree.",
            plan.instructions,
            plan.research_id,
            plan.corpus_revision,
            plan.research_blob,
            link.correlation_tag
        );
        let task = backend.create_scoped_task(
            title,
            &description,
            &link.correlation_tag,
            crew,
            &plan.context_files,
        )?;
        link.task_id = Some(
            task["id"]
                .as_str()
                .ok_or_else(|| {
                    Error::Invalid(
                        "Orbit task creation response omitted ID; reconcile before retry".into(),
                    )
                })?
                .into(),
        );
        journal.save(&key, &link)?;
        Ok(link)
    }
    /// A retry observes the existing durable run. It never silently dispatches twice.
    pub fn dispatch_work(&self, backend: &Backend, request_key: &str, base: &str) -> Result<Value> {
        let journal = self.store.operation_journal()?;
        let key = hash(request_key.as_bytes());
        let mut link = journal
            .read::<Link>(&key)?
            .ok_or_else(|| Error::Invalid("Unknown work request".into()))?;
        if link.workspace != backend.config().workspace
            || link.owner_machine_id != backend.config().owner_machine_id
            || backend.config().checkout.canonicalize()? != self.root()
        {
            return Err(Error::Invalid(
                "Task link belongs to another backend".into(),
            ));
        }
        let task_id = link
            .task_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("Task creation must be reconciled first".into()))?;
        let task = backend.task(task_id)?;
        if link.dispatch_attempted {
            if let Some(run) = correlated_run(&link, &task)? {
                return backend.run(run);
            }
            return Err(Error::Invalid(
                "Prior dispatch outcome is unknown; inspect Orbit runs before any new submission"
                    .into(),
            ));
        }
        if task["status"] != "backlog" {
            return Err(Error::Invalid(
                "Promote the task explicitly before dispatch".into(),
            ));
        }
        link.dispatch_attempted = true;
        journal.save(&key, &link)?;
        let submission = backend.dispatch(task_id, base)?;
        link.run_id = Some(
            submission["run_id"]
                .as_str()
                .ok_or_else(|| {
                    Error::Invalid("Submission omitted run ID; reconcile Orbit evidence".into())
                })?
                .into(),
        );
        journal.save(&key, &link)?;
        Ok(submission)
    }
    /// Read fresh task/run evidence for a previously linked request.
    pub fn work_status(&self, backend: &Backend, request_key: &str) -> Result<Value> {
        let link = self.link_for_backend(backend, request_key)?;
        let task_id = link
            .task_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("Task creation must be reconciled first".into()))?;
        let task = backend.task(task_id)?;
        let run_id = correlated_run(&link, &task)?;
        let run = run_id.map(|id| backend.run(id)).transpose()?;
        Ok(json!({"link":link,"task":task,"run":run,"source":"orbit",
            "observed_at_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| Error::Invalid(e.to_string()))?.as_millis()}))
    }

    pub fn promote_work(&self, backend: &Backend, request_key: &str) -> Result<Value> {
        let link = self.link_for_backend(backend, request_key)?;
        let id = link
            .task_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("Task creation must be reconciled first".into()))?;
        backend.promote(id)
    }

    /// Cancel only the run currently bound to this linked task. Never accept an
    /// arbitrary run ID supplied by a transport client.
    pub fn cancel_work(&self, backend: &Backend, request_key: &str) -> Result<Value> {
        let link = self.link_for_backend(backend, request_key)?;
        let task_id = link
            .task_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("Task creation must be reconciled first".into()))?;
        let task = backend.task(task_id)?;
        let run_id = correlated_run(&link, &task)?
            .ok_or_else(|| Error::Invalid("Linked task has no observed run".into()))?;
        backend.cancel(run_id)
    }

    /// Fetch the receipt from Orbit; callers cannot substitute task/run evidence.
    /// This validates existing delivery evidence without manufacturing an assessment.
    pub fn validate_work_result(
        &self,
        backend: &Backend,
        request_key: &str,
        receipt_path: &str,
        publication_ref: &str,
    ) -> Result<crate::receipt::AcceptedResult> {
        let link = self.link_for_backend(backend, request_key)?;
        let task_id = link
            .task_id
            .as_deref()
            .ok_or_else(|| Error::Invalid("Task creation must be reconciled first".into()))?;
        let task = backend.task(task_id)?;
        let run_id = correlated_run(&link, &task)?
            .ok_or_else(|| Error::Invalid("Linked task has no observed run".into()))?;
        let run = backend.run(run_id)?;
        let artifact = backend.artifact(task_id, receipt_path)?;
        let content = artifact["content"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Orbit receipt is not a UTF-8 artifact".into()))?;
        let receipt: crate::receipt::Receipt = serde_json::from_str(content)?;
        if receipt.record_id != link.research_id
            || receipt.task_id != task_id
            || receipt.run_id != run_id
        {
            return Err(Error::Invalid(
                "Receipt does not belong to this linked investigation".into(),
            ));
        }
        self.validate_receipt(
            &receipt,
            &task,
            &run,
            &link.workspace,
            &link.owner_machine_id,
            publication_ref,
        )
    }

    fn link_for_backend(&self, backend: &Backend, request_key: &str) -> Result<Link> {
        let link: Link = self
            .store
            .operation_journal()?
            .read(&hash(request_key.as_bytes()))?
            .ok_or_else(|| Error::Invalid("Unknown work request".into()))?;
        if link.workspace != backend.config().workspace
            || link.owner_machine_id != backend.config().owner_machine_id
            || backend.config().checkout.canonicalize()? != self.root()
        {
            return Err(Error::Invalid(
                "Task link belongs to another backend".into(),
            ));
        }
        Ok(link)
    }

    pub fn work_links(&self) -> Result<Vec<Link>> {
        self.store.operation_journal()?.list()
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn correlated_run<'a>(link: &'a Link, task: &'a Value) -> Result<Option<&'a str>> {
    let current = task["job_run_id"].as_str();
    if let Some(expected) = link.run_id.as_deref() {
        if current != Some(expected) {
            return Err(Error::Invalid(
                "Linked run differs from current Orbit task correlation; reconcile before acting"
                    .into(),
            ));
        }
    }
    Ok(current)
}
