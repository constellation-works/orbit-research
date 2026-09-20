//! External, explicitly routed Orbit CLI. No Orbit implementation crates or stores.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::config::BackendConfig;

/// Entries are certified by a release's adapter integration tests, not a version guess.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub binary_sha256: String,
    pub version: String,
    pub platform: String,
    pub operations: Vec<String>,
}

pub struct OrbitBackend {
    config: BackendConfig,
    compatibility: Vec<Compatibility>,
    timeout: Duration,
}

impl OrbitBackend {
    pub fn new(config: BackendConfig, compatibility: Vec<Compatibility>) -> Result<Self> {
        if !config.executable.is_absolute()
            || !config.checkout.is_absolute()
            || !config.workspace.starts_with("ws_")
            || config.owner_machine_id.is_empty()
        {
            return Err(Error::Invalid("Backend needs an absolute executable and checkout, registered ws_ selector, and owner machine ID".into()));
        }
        Ok(Self {
            config,
            compatibility,
            timeout: Duration::from_secs(30),
        })
    }

    pub fn config(&self) -> &BackendConfig {
        &self.config
    }

    pub fn inspect(&self) -> Result<Value> {
        self.verify("observe")?;
        let identity = self.json(&[
            "workspace".into(),
            "show".into(),
            "--format".into(),
            "json".into(),
        ])?;
        if identity["registered"] != true
            || identity["workspace"]["id"] != self.config.workspace
            || identity["workspace"]["owner_machine_id"] != self.config.owner_machine_id
        {
            return Err(Error::Invalid(
                "Orbit returned a different workspace or owner".into(),
            ));
        }
        let actual = identity["checkout"]["repo_root"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Orbit omitted checkout identity".into()))?;
        if fs::canonicalize(actual)? != self.config.checkout.canonicalize()? {
            return Err(Error::Invalid(
                "Orbit checkout differs from the explicitly configured owner".into(),
            ));
        }
        Ok(identity)
    }

    pub fn task(&self, id: &str) -> Result<Value> {
        self.inspect()?;
        let task = self.tool(
            "orbit.task.show",
            json!({
                "id": id,
                "fields": [
                    "id", "title", "status", "job_run_id", "job_run_host", "artifacts", "comments"
                ],
            }),
        )?;
        if task["id"] != id {
            return Err(Error::Invalid("Orbit returned the wrong task".into()));
        }
        Ok(task)
    }

    pub fn run(&self, id: &str) -> Result<Value> {
        self.inspect()?;
        let output = self.json(&["run".into(), "show".into(), id.into(), "--json".into()])?;
        let run = output
            .get("run")
            .ok_or_else(|| Error::Invalid("Orbit run response shape changed".into()))?;
        if run["run_id"] != id {
            return Err(Error::Invalid("Orbit returned the wrong run".into()));
        }
        Ok(run.clone())
    }

    pub fn artifact(&self, task: &str, path: &str) -> Result<Value> {
        self.task(task)?;
        self.tool("orbit.task.artifact.get", json!({"id":task,"path":path}))
    }

    /// The caller records its intent BEFORE calling: public task.add has no idempotency key.
    /// An uncertain response must be reconciled by correlation tag, never blindly retried.
    pub fn create_task(
        &self,
        title: &str,
        description: &str,
        correlation_tag: &str,
        crew: &str,
    ) -> Result<Value> {
        self.verify("task_add")?;
        self.inspect()?;
        if !correlation_tag.starts_with("research-request:") || correlation_tag.len() > 120 {
            return Err(Error::Invalid(
                "Invalid research request correlation".into(),
            ));
        }
        self.tool(
            "orbit.task.add",
            json!({
                "title": title,
                "description": description,
                "complexity": "medium",
                "type": "feature",
                "crew": crew,
                "tags": [correlation_tag],
                "acceptance_criteria": ["Publish the scoped research result with Question, Method, Result, Limitations and Next, preserving controls and uncertainty."],
                "fields": ["id", "status", "tags"],
            }),
        )
    }

    pub fn create_scoped_task(
        &self,
        title: &str,
        description: &str,
        correlation_tag: &str,
        crew: &str,
        context_files: &[String],
    ) -> Result<Value> {
        self.verify("task_add")?;
        self.inspect()?;
        if !correlation_tag.starts_with("research-request:")
            || correlation_tag.len() > 120
            || context_files.is_empty()
        {
            return Err(Error::Invalid(
                "Scoped task needs correlation and explicit write paths".into(),
            ));
        }
        self.tool(
            "orbit.task.add",
            json!({
                "title": title,
                "description": description,
                "complexity": "medium",
                "type": "feature",
                "crew": crew,
                "tags": [correlation_tag],
                "context_files": context_files,
                "allow_missing_context": true,
                "acceptance_criteria": ["Deliver the scoped findings and artifact digests without changing records outside the declared write paths; retain failed controls and limitations."],
                "fields": ["id", "status", "tags"],
            }),
        )
    }

    pub fn promote(&self, id: &str) -> Result<Value> {
        self.verify("promote")?;
        self.task(id)?;
        self.tool(
            "orbit.task.update",
            json!({
                "id": id,
                    "status": "backlog",
                    "comment": "Explicitly approved for research execution through orbit-research."
            }),
        )
    }

    pub fn correlated_tasks(&self, correlation_tag: &str) -> Result<Vec<Value>> {
        self.inspect()?;
        let result = self.tool(
            "orbit.task.list",
            json!({"tag":correlation_tag,"limit":100}),
        )?;
        if result["truncated"] != false {
            return Err(Error::Invalid(
                "Task reconciliation is truncated; operator review required".into(),
            ));
        }
        result["tasks"]
            .as_array()
            .cloned()
            .ok_or_else(|| Error::Invalid("Orbit task list response shape changed".into()))
    }

    /// Explicit submission. Orbit enforces the actual operator/managed-run boundary.
    /// No caller role or permission override is injected by this adapter.
    pub fn dispatch(&self, task: &str, base: &str) -> Result<Value> {
        self.prepare_dispatch(task, base)?.submit()
    }

    /// Complete all deterministic checks before the caller records uncertainty.
    pub(crate) fn prepare_dispatch<'a>(
        &'a self,
        task: &str,
        base: &str,
    ) -> Result<PreparedDispatch<'a>> {
        validate_base(base)?;
        self.verify("ship")?;
        let current = self.task(task)?;
        if current["job_run_id"].is_string() {
            return Err(Error::Invalid(
                "Task already has run history: reconcile before dispatch".into(),
            ));
        }
        if current["status"] != "backlog" {
            return Err(Error::Invalid(
                "Only an explicitly promoted task can be dispatched".into(),
            ));
        }
        Ok(PreparedDispatch {
            backend: self,
            task: task.into(),
            base: base.into(),
        })
    }

    pub fn cancel(&self, id: &str) -> Result<Value> {
        self.verify("cancel")?;
        self.run(id)?;
        self.json(&[
            "run".into(),
            "cancel".into(),
            id.into(),
            "--confirm".into(),
            "--json".into(),
        ])
    }

    fn tool(&self, name: &str, mut input: Value) -> Result<Value> {
        input["workspace"] = json!(self.config.workspace);
        self.json(&[
            "tool".into(),
            "run".into(),
            name.into(),
            "--input".into(),
            input.to_string(),
            "--format".into(),
            "json".into(),
        ])
    }

    fn verify(&self, operation: &str) -> Result<()> {
        let hash = format!("{:x}", Sha256::digest(fs::read(&self.config.executable)?));
        let entry = self.compatibility.iter().find(|entry| {
            entry.binary_sha256 == hash
                && entry.platform == std::env::consts::OS
                && entry.operations.iter().any(|supported| supported == operation)
        }).ok_or_else(|| Error::Invalid(format!(
            "This Orbit binary is not certified for {operation}; local research remains available"
        )))?;
        let version = self.output(&["--version".into()], false)?;
        if version.trim() != entry.version {
            return Err(Error::Invalid(
                "Orbit version and certified binary identity differ".into(),
            ));
        }
        Ok(())
    }

    fn json(&self, args: &[String]) -> Result<Value> {
        serde_json::from_str(&self.output(args, true)?).map_err(Error::from)
    }

    fn output(&self, args: &[String], routed: bool) -> Result<String> {
        let mut stdout = tempfile::tempfile()?;
        let mut stderr = tempfile::tempfile()?;
        let mut command = Command::new(&self.config.executable);
        command
            .current_dir(&self.config.checkout)
            .stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?);
        if routed {
            command.arg("--workspace").arg(&self.config.workspace);
        }
        command.args(args);
        // Preserve the caller's environment, including Orbit's managed-run restrictions.
        // A child command is never granted permissions merely because the app requested it.
        let mut child = command.spawn()?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > self.timeout
                || stdout.metadata()?.len() > 2_000_000
                || stderr.metadata()?.len() > 2_000_000
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Invalid("Orbit response interrupted or exceeded limits; reconcile durable task/run state before retrying".into()));
            }
            thread::sleep(Duration::from_millis(20));
        };
        stdout.seek(SeekFrom::Start(0))?;
        stderr.seek(SeekFrom::Start(0))?;
        let mut out = String::new();
        let mut err = String::new();
        stdout.take(2_000_001).read_to_string(&mut out)?;
        stderr.take(2_000_001).read_to_string(&mut err)?;
        if !status.success() {
            return Err(Error::Invalid(format!(
                "Orbit refused the request: {}",
                err.trim()
            )));
        }
        if out.len() > 2_000_000 {
            return Err(Error::Invalid("Orbit output exceeds adapter limit".into()));
        }
        Ok(out)
    }
}

/// Only a successful preflight can construct this submission capability.
pub(crate) struct PreparedDispatch<'a> {
    backend: &'a OrbitBackend,
    task: String,
    base: String,
}

impl PreparedDispatch<'_> {
    pub(crate) fn submit(self) -> Result<Value> {
        self.backend.json(&[
            "run".into(),
            "ship".into(),
            self.task,
            "--base".into(),
            self.base,
            "--mode".into(),
            "pr".into(),
            "--json".into(),
        ])
    }
}

fn validate_base(base: &str) -> Result<()> {
    if base.is_empty()
        || base == "@"
        || base == "HEAD"
        || base.starts_with('-')
        || base.starts_with('/')
        || base.ends_with('/')
        || base.ends_with('.')
        || base.contains("..")
        || base.contains("@{")
        || base.contains("//")
        || base
            .bytes()
            .any(|c| c <= b' ' || c == 127 || b"~^:?*[\\".contains(&c))
        || base
            .split('/')
            .any(|part| part.starts_with('.') || part.ends_with(".lock"))
    {
        return Err(Error::Invalid(
            "Dispatch base must be a valid, non-option Git branch reference".into(),
        ));
    }
    Ok(())
}
