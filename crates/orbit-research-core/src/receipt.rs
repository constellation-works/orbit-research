//! A run finishing is not scientific acceptance. Acceptance binds Orbit evidence
//! to the exact published corpus commit and artifacts, without rewriting a verdict.
use crate::{Error, Research as Corpus, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub workspace: String,
    pub owner_machine_id: String,
    pub task_id: String,
    pub run_id: String,
    pub record_id: String,
    pub record_path: String,
    pub corpus_commit: String,
    pub record_blob: String,
    pub artifacts: Vec<Artifact>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
pub struct AcceptedResult {
    pub record_id: String,
    pub task_id: String,
    pub run_id: String,
    pub corpus_commit: String,
    pub record_blob: String,
}
impl Corpus {
    /// Caller obtains `receipt`, `task` and `run` from the configured Orbit backend.
    /// A local cache or agent response is never supplied as authoritative evidence.
    pub fn validate_receipt(
        &self,
        receipt: &Receipt,
        task: &Value,
        run: &Value,
        workspace: &str,
        owner_machine_id: &str,
        publication_ref: &str,
    ) -> Result<AcceptedResult> {
        if receipt.schema_version != 1
            || receipt.workspace != workspace
            || receipt.owner_machine_id != owner_machine_id
        {
            return Err(Error::Invalid(
                "Receipt belongs to a different backend or schema".into(),
            ));
        }
        if task["id"] != receipt.task_id
            || task["job_run_id"] != receipt.run_id
            || run["run_id"] != receipt.run_id
            || run["state"] != "success"
        {
            return Err(Error::Invalid(
                "Receipt does not match a successful authoritative task/run binding".into(),
            ));
        }
        if !matches!(task["status"].as_str(), Some("review" | "done")) {
            return Err(Error::Invalid(
                "Task has not reached delivery review".into(),
            ));
        }
        if !matches!(receipt.corpus_commit.len(), 40 | 64)
            || !receipt.corpus_commit.bytes().all(|b| b.is_ascii_hexdigit())
            || !publication_ref.starts_with("refs/remotes/")
        {
            return Err(Error::Invalid(
                "Receipt needs a full commit ID and explicit publication ref".into(),
            ));
        }
        let published = self
            .store
            .published(&receipt.corpus_commit, publication_ref)?;
        if !published {
            return Err(Error::Invalid(
                "Result commit is not reachable from the observed publication ref".into(),
            ));
        }
        if run["executed_on"]["machine_id"] != owner_machine_id {
            return Err(Error::Invalid(
                "Run execution host does not match the configured owner".into(),
            ));
        }
        safe_relative(&receipt.record_path)?;
        let record_directory = std::path::Path::new(&receipt.record_path)
            .parent()
            .ok_or_else(|| Error::Invalid("Missing record directory".into()))?;
        let dirname = record_directory
            .file_name()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        let owner_directory = self.store.schema()["x-observatory"]["kinds"]["R"]["directory"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Missing owner research directory".into()))?;
        if record_directory.parent() != Some(std::path::Path::new(owner_directory))
            || !dirname.starts_with(&format!("{}-", receipt.record_id))
            || std::path::Path::new(&receipt.record_path)
                .file_name()
                .and_then(|p| p.to_str())
                != Some("README.md")
        {
            return Err(Error::Invalid(
                "Receipt result path does not match owner layout".into(),
            ));
        }

        let blob = self
            .store
            .committed_blob(&receipt.corpus_commit, &receipt.record_path)?;
        if blob != receipt.record_blob {
            return Err(Error::Invalid("Result blob differs from receipt".into()));
        }
        let bytes = self
            .store
            .committed_bytes(&receipt.corpus_commit, &receipt.record_path)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::Invalid("Result is not UTF-8 Markdown".into()))?;
        let rest = text
            .strip_prefix("---\n")
            .ok_or_else(|| Error::Invalid("Result lacks frontmatter".into()))?;
        let (front, body) = rest
            .split_once("\n---\n")
            .ok_or_else(|| Error::Invalid("Result frontmatter is unclosed".into()))?;
        let metadata: Value = serde_yaml::from_str(front)?;
        let schema = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&self.store.schema())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if !schema.is_valid(&metadata)
            || metadata["id"] != receipt.record_id
            || !receipt.record_id.starts_with('R')
            || metadata["status"] != "done"
            || metadata["orbit"]["task"] != receipt.task_id
            || metadata["orbit"]["run"] != receipt.run_id
        {
            return Err(Error::Invalid(
                "Result metadata does not bind the delivered research record to this run".into(),
            ));
        }
        let mut sections = std::collections::BTreeMap::new();
        let mut current = None;
        for line in body.lines() {
            if let Some(name) = line.strip_prefix("## ") {
                current = Some(name.trim().to_owned());
                sections
                    .entry(name.trim().to_owned())
                    .or_insert(String::new());
            } else if let Some(name) = &current {
                sections
                    .entry(name.clone())
                    .or_insert(String::new())
                    .push_str(&format!("{line}\n"));
            }
        }
        for name in ["Question", "Method", "Result", "Limitations", "Next"] {
            let content = sections.get(name).map(|s| s.trim()).unwrap_or("");
            if content.is_empty()
                || content == "Pending."
                || content == "Not yet run."
                || content == "Await dispatch."
                || (content.starts_with("<!--") && content.ends_with("-->"))
            {
                return Err(Error::Invalid(format!(
                    "Result section {name} is missing or still a scaffold"
                )));
            }
        }
        let directory = std::path::Path::new(&receipt.record_path)
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid result path".into()))?;
        let mut seen = std::collections::BTreeSet::new();
        for artifact in &receipt.artifacts {
            if !seen.insert(&artifact.path) {
                return Err(Error::Invalid("Duplicate receipt artifact".into()));
            }

            safe_relative(&artifact.path)?;
            if !std::path::Path::new(&artifact.path).starts_with(directory) {
                return Err(Error::Invalid(
                    "Receipt artifact escapes its research item".into(),
                ));
            }
            let digest = format!(
                "{:x}",
                Sha256::digest(
                    self.store
                        .committed_bytes(&receipt.corpus_commit, &artifact.path)?
                )
            );
            if digest != artifact.sha256 {
                return Err(Error::Invalid(format!(
                    "Artifact digest mismatch: {}",
                    artifact.path
                )));
            }
        }
        Ok(AcceptedResult {
            record_id: receipt.record_id.clone(),
            task_id: receipt.task_id.clone(),
            run_id: receipt.run_id.clone(),
            corpus_commit: receipt.corpus_commit.clone(),
            record_blob: receipt.record_blob.clone(),
        })
    }
}

fn safe_relative(path: &str) -> Result<()> {
    if path.is_empty()
        || std::path::Path::new(path)
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid("Invalid receipt path".into()));
    }
    Ok(())
}
