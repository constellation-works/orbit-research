//! Orbit plugin `exec` backend transport (`orbit-tool`, invoked as
//! `orbit-research orbit-tool`). One process per call: read the host's single
//! stdin JSON request, dispatch through Core, write the single stdout JSON
//! reply. All research operations delegate to Core, as MCP does; this
//! transport speaks the plugin envelope instead of JSON-RPC and always exits
//! `0`, carrying failure in `ok:false` rather than a process exit code.
use crate::panels;
use orbit_research_core::{
    Error, Research, Result,
    api::Application,
    application::{Operation, acceptance::Acceptance, operations::check_request_key},
    delivery::{DeliveryReport, Expected},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bound the complete exec envelope, including host context, before parsing.
const MAX_PLUGIN_REQUEST_BYTES: u64 = 1024 * 1024;

/// Map a plugin tool call to a Core operation. The host may send the bare
/// verb, the local-development namespace (`research.list`) or the verified
/// first-party namespace (`orbit.research.list`); only the trailing verb is
/// wire contract here, so all three resolve the same way.
fn operation_for_verb(verb: &str) -> Option<Operation> {
    match verb {
        "list" => Some(Operation::List),
        "show" => Some(Operation::Show),
        "check" => Some(Operation::Check),
        "plan" => Some(Operation::Plan),
        _ => None,
    }
}

fn verb(tool: &str) -> &str {
    tool.rsplit('.').next().unwrap_or(tool)
}

fn error_code(error: &Error) -> &'static str {
    match error {
        Error::InvalidInput(_) => "invalid_request",
        Error::NotFound(_) => "record_not_found",
        Error::Conflict(_) => "conflict",
        Error::Refused(_) => "refused",
        Error::Acceptance(_) => "acceptance_required",
        Error::Invalid(_) | Error::Corpus(_) => "corpus_unavailable",
        Error::Internal(_) => "internal",
        Error::Io(_) => "io_error",
        Error::Json(_) | Error::Yaml(_) => "internal",
    }
}

pub(crate) fn error_envelope(code: &str, message: impl Into<String>) -> Value {
    json!({"ok": false, "error": {"code": code, "message": message.into(), "retryable": false}})
}

/// The failure reply for a Core error. A corpus that fails validation also
/// carries every problem as a `{path, field, message}` object under `problems`.
pub(crate) fn error_reply(error: &Error) -> Value {
    let mut reply = error_envelope(error_code(error), error.to_string());
    if let Error::Corpus(issues) = error {
        reply["error"]["problems"] = json!(issues);
    }
    reply
}

/// The plugin protocol's `version` tool reports Core's own version
/// unconditionally: a corpus-independent health signal, so it still answers
/// when the bound workspace has no valid corpus.
fn version_output() -> Value {
    json!({"ok": true, "output": {"core_version": orbit_research_core::VERSION}})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionInput {}

/// A reference to an Orbit task, as returned by `orbit.task.list`/`orbit.task.add`.
pub(crate) struct TaskRef {
    pub(crate) id: String,
}

/// The task fields `accept` gates on: `orbit.task.show` projection of
/// `status` and `job_run_id`. The delivering run's id comes from the task,
/// never from the caller's own call context — `accept` targets an
/// explicitly named task, which need not be the one the caller is running
/// under.
pub(crate) struct TaskState {
    pub(crate) status: String,
    pub(crate) job_run_id: Option<String>,
}

/// The host callbacks `link` and `accept` need. Production calls spawn
/// `orbit tool run` (the only way a sandboxed `exec` backend reaches Orbit);
/// tests substitute an in-memory fake so goldens don't need a real Orbit
/// installation.
pub(crate) trait TaskHost {
    fn list_by_tag(&self, workspace: &str, tag: &str) -> Result<Vec<TaskRef>>;
    fn create(
        &self,
        workspace: &str,
        tag: &str,
        title: &str,
        description: &str,
        acceptance_criteria: &[String],
        context_files: &[String],
    ) -> Result<TaskRef>;
    /// Current status and delivering run id for a task, via `orbit.task.show`.
    fn task_state(&self, id: &str) -> Result<TaskState>;
    /// The named task artifact's parsed JSON content, if it has been stored.
    fn get_artifact(&self, id: &str, path: &str) -> Result<Option<Value>>;
    /// Store `source_path`'s bytes as the task artifact named `path`, via
    /// `orbit.task.artifact.put`. `source_path` must resolve inside the
    /// bound workspace: that callback reads real bytes off disk, never
    /// inline content, outside the ssh-mcp spoke connector.
    fn put_artifact(&self, source_path: &Path, id: &str, path: &str) -> Result<()>;
}

/// Spawns the host's own `orbit` binary, exactly as a person would run
/// `orbit tool run <name> --input <json>`. Reachable without a
/// `requires.programs` declaration: a callback to a tool granted under
/// `permissions.orbit_tools` is host-mediated, not an arbitrary subprocess.
pub(crate) struct OrbitCliTaskHost;

/// The `orbit` executable: `ORBIT_BIN` when set, else `orbit` on `PATH`. The
/// plugin transport and the writer's acceptance lookup resolve it identically.
pub(crate) fn orbit_binary() -> String {
    std::env::var("ORBIT_BIN").unwrap_or_else(|_| "orbit".into())
}

fn run_orbit_tool(name: &str, input: &Value) -> Result<Value> {
    run_orbit_tool_with(&orbit_binary(), None, name, input)
}

/// Run `orbit tool run <name> --input <json>` with `orbit` as the executable
/// and, when given, `cwd` as the working directory (how Orbit selects the
/// workspace outside a plugin sandbox).
pub(crate) fn run_orbit_tool_with(
    orbit: &str,
    cwd: Option<&Path>,
    name: &str,
    input: &Value,
) -> Result<Value> {
    let mut command = Command::new(orbit);
    command.args(["tool", "run", name, "--input", &input.to_string()]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().map_err(|error| {
        Error::Internal(format!("failed to invoke `orbit tool run {name}`: {error}"))
    })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!(
            "`orbit tool run {name}` failed: {}",
            message.trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        Error::Internal(format!(
            "`orbit tool run {name}` returned non-JSON output: {error}"
        ))
    })
}

/// The text of a task's stored artifact, or `None` when the task has never
/// stored one. `orbit.task.artifact.get` errors on a missing artifact, so list
/// first with `orbit.task.show` and keep a never-stored artifact distinct from
/// a spurious failure.
pub(crate) fn read_task_artifact(
    orbit: &str,
    cwd: Option<&Path>,
    id: &str,
    path: &str,
) -> Result<Option<String>> {
    let listed = run_orbit_tool_with(
        orbit,
        cwd,
        "orbit.task.show",
        &json!({"id": id, "fields": ["artifacts"]}),
    )?;
    let artifacts = listed.get("artifacts").cloned().unwrap_or(listed);
    let present = artifacts
        .as_array()
        .into_iter()
        .flatten()
        .any(|artifact| artifact.get("path").and_then(Value::as_str) == Some(path));
    if !present {
        return Ok(None);
    }
    let output = run_orbit_tool_with(
        orbit,
        cwd,
        "orbit.task.artifact.get",
        &json!({"id": id, "path": path}),
    )?;
    output
        .get("content")
        .and_then(Value::as_str)
        .map(|content| Some(content.to_owned()))
        .ok_or_else(|| {
            Error::Internal(format!(
                "orbit.task.artifact.get did not return text content for {path}"
            ))
        })
}

impl TaskHost for OrbitCliTaskHost {
    fn list_by_tag(&self, workspace: &str, tag: &str) -> Result<Vec<TaskRef>> {
        let output = run_orbit_tool(
            "orbit.task.list",
            &json!({"workspace": workspace, "tag": tag}),
        )?;
        let tasks = output
            .get("tasks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tasks
            .iter()
            .filter_map(|task| task.get("id").and_then(Value::as_str))
            .map(|id| TaskRef { id: id.into() })
            .collect())
    }

    fn create(
        &self,
        workspace: &str,
        tag: &str,
        title: &str,
        description: &str,
        acceptance_criteria: &[String],
        context_files: &[String],
    ) -> Result<TaskRef> {
        let output = run_orbit_tool(
            "orbit.task.add",
            &json!({
                "workspace": workspace,
                "title": title,
                "description": description,
                "complexity": "medium",
                "acceptance_criteria": acceptance_criteria,
                "context_files": context_files,
                "allow_missing_context": true,
                "tags": [tag],
            }),
        )?;
        let id = output
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Internal("orbit.task.add did not return a task id".into()))?;
        Ok(TaskRef { id: id.into() })
    }

    fn task_state(&self, id: &str) -> Result<TaskState> {
        let output = run_orbit_tool(
            "orbit.task.show",
            &json!({"id": id, "fields": ["status", "job_run_id"]}),
        )?;
        let status = output
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Internal("orbit.task.show did not return status".into()))?
            .to_owned();
        let job_run_id = output
            .get("job_run_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(TaskState { status, job_run_id })
    }

    fn get_artifact(&self, id: &str, path: &str) -> Result<Option<Value>> {
        let Some(content) = read_task_artifact(&orbit_binary(), None, id, path)? else {
            return Ok(None);
        };
        serde_json::from_str(&content)
            .map(Some)
            .map_err(|error| Error::Internal(format!("stored {path} is not valid JSON: {error}")))
    }

    fn put_artifact(&self, source_path: &Path, id: &str, path: &str) -> Result<()> {
        run_orbit_tool(
            "orbit.task.artifact.put",
            &json!({
                "id": id,
                "source_path": source_path.to_string_lossy(),
                "path": path,
            }),
        )?;
        Ok(())
    }
}

// `link`'s own input contract. Not a Core `Operation`: only this transport
// can reach the `orbit.task.add`/`orbit.task.list` callbacks, and Core never
// shells out to Orbit (see ARCHITECTURE.md). `plan` emits `context_files`, so
// `link` accepts it to let that output pass through unchanged, but only when it
// equals a scope `plan` derives for the reserved research item (the research
// directory, one contribution unit's `code/` and `artifacts/` paths, or the
// synthesis files; `Application::link_intent` decides), so caller input never
// widens a task past the reserved R. Omitted, the scope is the research
// directory. A plain (non-doc) comment: schemars would lift a
// doc comment into the schema's `description`, which schemas/link.request.json
// (checked for drift in src/tests/plugin.rs) does not carry.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkInput {
    #[schemars(regex(pattern = "^R[0-9]{3}$"))]
    research_id: String,
    #[schemars(length(min = 1, max = 256))]
    request_key: String,
    #[schemars(length(min = 1))]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    context_files: Option<Vec<String>>,
}

fn link(workspace_root: &Path, input: Value, host: &dyn TaskHost) -> Result<Value> {
    let mut input: LinkInput =
        serde_json::from_value(input).map_err(|error| Error::InvalidInput(error.to_string()))?;
    if input.title.trim().is_empty() {
        return Err(Error::InvalidInput(
            "`title` must contain non-whitespace text; supply a title for the investigation".into(),
        ));
    }
    // Orbit refuses blank task descriptions. Honor link's optional field by
    // deriving a usable description before persisting any submission intent.
    if input.description.trim().is_empty() {
        input.description = input.title.clone();
    }
    // An unusable key is the caller's mistake: say so before touching the corpus.
    check_request_key(&input.request_key)?;
    let app = Application::local(workspace_root)?;
    app.require_prepared_operations()?;
    let preparation = app.link_intent(
        &input.request_key,
        &input.research_id,
        input.context_files.as_deref(),
    )?;
    let workspace = workspace_root.to_string_lossy();
    let tag = &preparation.link.correlation_tag;
    let matches = host.list_by_tag(&workspace, tag)?;
    match matches.len() {
        1 => {
            let task_id = &matches[0].id;
            app.link_confirm(&input.request_key, task_id)?;
            Ok(json!({
                "research_id": input.research_id,
                "task_id": task_id,
                "created": false,
            }))
        }
        0 if preparation.is_new => {
            let task = host.create(
                &workspace,
                tag,
                &input.title,
                &input.description,
                &input.acceptance_criteria,
                &preparation.context_files,
            )?;
            app.link_confirm(&input.request_key, &task.id)?;
            Ok(json!({
                "research_id": input.research_id,
                "task_id": task.id,
                "created": true,
            }))
        }
        0 => Err(Error::Conflict(format!(
            "A prior link attempt for this request key was recorded, but no Orbit task tagged `{tag}` exists; resolve manually before retrying"
        ))),
        count => Err(Error::Conflict(format!(
            "{count} Orbit tasks are tagged `{tag}`; expected exactly one"
        ))),
    }
}

fn link_output(workspace_root: &Path, input: Value, host: &dyn TaskHost) -> Value {
    match link(workspace_root, input, host) {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => error_reply(&error),
    }
}

// `validate`'s own input contract. Not a Core `Operation`: every operation
// opens the bound workspace, and `validate` must read only the checkout it is
// given, because a job step's `context.workspace_root` is the primary checkout
// even while the run's work sits in its worktree. A plain comment for the same
// schema-description reason as `LinkInput`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ValidateInput {
    #[schemars(length(min = 1))]
    path: String,
    #[schemars(regex(pattern = "^R[0-9]{3}$"))]
    research_id: Option<String>,
}

fn validate(input: Value, task: &str, run: &str) -> Result<DeliveryReport> {
    let input: ValidateInput =
        serde_json::from_value(input).map_err(|error| Error::InvalidInput(error.to_string()))?;
    let path = Path::new(&input.path);
    if !path.is_absolute() {
        return Err(Error::InvalidInput(format!(
            "path must be an absolute checkout path: {}",
            input.path
        )));
    }
    Research::open(path)?.validate_delivery(&Expected {
        research_id: input.research_id.as_deref(),
        task,
        run,
    })
}

/// The delivery gate. Any finding is `ok:false`: Orbit fails a
/// `plugin.tool_call` step only on `ok:false`, never on `valid:false` output.
/// The first finding's reason is the error code; the message lists them all.
fn validate_output(request: &Value, input: Value) -> Value {
    let context = |field: &str| {
        request
            .pointer(&format!("/context/{field}"))
            .and_then(Value::as_str)
    };
    let (Some(task), Some(run)) = (context("task_id"), context("job_run_id")) else {
        return error_envelope(
            "run_context_required",
            "validate checks a run's delivery and needs the run's task_id and job_run_id context",
        );
    };
    match validate(input, task, run) {
        Ok(report) if report.valid() => json!({
            "ok": true,
            "output": {
                "valid": true,
                "research_id": report.research_id,
                "path": report.path,
                "blob": report.blob,
                "revision": report.revision,
                "unverified_inputs": report.unverified_inputs,
            },
        }),
        Ok(report) => error_envelope(
            report.findings[0].reason.as_str(),
            report
                .findings
                .iter()
                .map(|finding| format!("{}: {}", finding.reason.as_str(), finding.message))
                .collect::<Vec<_>>()
                .join("; "),
        ),
        Err(error) => error_reply(&error),
    }
}

// `accept`'s own input contract. Not a Core `Operation`: only this transport
// reaches the `orbit.task.show`/`orbit.task.artifact.{get,put}` callbacks,
// and Core never shells out to Orbit (see ARCHITECTURE.md). `task_id` is
// explicit caller input, unlike `validate`'s host-attested `context.task_id`:
// `accept` targets a specific already-delivered task, which need not be the
// task the caller is running under. A plain comment for the same
// schema-description reason as `LinkInput`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AcceptInput {
    #[schemars(length(min = 1))]
    task_id: String,
    #[schemars(regex(pattern = "^R[0-9]{3}$"))]
    research_id: String,
}

pub(crate) const ACCEPTANCE_ARTIFACT_PATH: &str = "research-acceptance.json";
/// Scratch write root for `accept`'s own staged artifact file, granted under
/// `permissions.fs.write`. Never `.orbit`/`.git`: the sandbox refuses any
/// write root that reaches workspace metadata.
const ACCEPT_SCRATCH_DIR: &str = ".orbit-research-tmp";

struct ScratchDirectory {
    path: PathBuf,
    #[cfg(unix)]
    directory: fs::File,
}

fn scratch_directory(workspace_root: &Path) -> Result<ScratchDirectory> {
    let scratch = workspace_root.canonicalize()?.join(ACCEPT_SCRATCH_DIR);
    let directory = fs::DirBuilder::new();
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::DirBuilderExt;
        let mut directory = directory;
        directory.mode(0o700);
        directory
    };
    match directory.create(&scratch) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(&scratch)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Refused(format!(
            "Acceptance scratch path must be an ordinary directory: {}",
            scratch.display()
        )));
    }
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::OpenOptionsExt;
        // Refuse a symlink substituted between the metadata check and open.
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(&scratch)?
    };
    Ok(ScratchDirectory {
        path: scratch,
        #[cfg(unix)]
        directory,
    })
}

struct AcceptOutcome {
    acceptance: Acceptance,
    /// Whether this call wrote the artifact; `false` for an idempotent retry
    /// that found matching evidence already stored.
    recorded: bool,
}

/// The evidence-identity fields on which a stored acceptance differs from the
/// one a retry would record. Identity is research id, README blob, run id and
/// artifact digests; `commit` is not part of it.
fn evidence_differences(stored: &Acceptance, candidate: &Acceptance) -> Vec<&'static str> {
    let mut differing = Vec::new();
    if stored.research_id != candidate.research_id {
        differing.push("research_id");
    }
    if stored.blob != candidate.blob {
        differing.push("blob");
    }
    if stored.run_id != candidate.run_id {
        differing.push("run_id");
    }
    if stored.artifact_digests != candidate.artifact_digests {
        differing.push("artifact_digests");
    }
    differing
}

/// Persist (or idempotently confirm) `research-acceptance.json` for a
/// validated delivery. A retry whose evidence identity matches returns the
/// stored acceptance whatever the current HEAD. Only reachable once `accept_output` has confirmed the
/// task is `review`/`done` and `validate_delivery` found nothing.
fn accept_record(
    workspace_root: &Path,
    research_id: &str,
    report: &DeliveryReport,
    run: &str,
    task: &str,
    host: &dyn TaskHost,
) -> Result<AcceptOutcome> {
    let candidate = Acceptance {
        research_id: research_id.to_owned(),
        commit: report.revision.clone(),
        blob: report.blob.clone().ok_or_else(|| {
            Error::Internal("a delivery report with no findings always names a blob".into())
        })?,
        run_id: run.to_owned(),
        artifact_digests: report.artifact_digests.clone(),
    };
    if let Some(existing) = host.get_artifact(task, ACCEPTANCE_ARTIFACT_PATH)? {
        let existing: Acceptance = serde_json::from_value(existing).map_err(|error| {
            Error::Internal(format!(
                "{task}'s stored {ACCEPTANCE_ARTIFACT_PATH} is not valid: {error}"
            ))
        })?;
        let differing = evidence_differences(&existing, &candidate);
        if differing.is_empty() {
            // `commit` is deliberately outside the identity: it is the HEAD of
            // the first accept, and any later corpus commit moves HEAD without
            // touching the evidence. The stored value is returned unchanged.
            return Ok(AcceptOutcome {
                acceptance: existing,
                recorded: false,
            });
        }
        return Err(Error::Conflict(format!(
            "{task} already carries {ACCEPTANCE_ARTIFACT_PATH} with different evidence ({}); refusing to overwrite recorded acceptance",
            differing.join(", ")
        )));
    }
    let scratch = scratch_directory(workspace_root)?;
    let mut scratch_file = tempfile::Builder::new()
        .prefix("research-acceptance-")
        .suffix(".json")
        .tempfile_in(&scratch.path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = scratch.directory.metadata()?;
        let current = fs::symlink_metadata(&scratch.path)?;
        if current.file_type().is_symlink()
            || (opened.dev(), opened.ino()) != (current.dev(), current.ino())
        {
            return Err(Error::Refused(format!(
                "Acceptance scratch directory changed while staging: {}",
                scratch.path.display()
            )));
        }
    }
    serde_json::to_writer_pretty(scratch_file.as_file_mut(), &candidate)?;
    scratch_file.flush()?;
    // The uniquely created, owner-only file stays alive through the callback;
    // its guard removes only this call's file on success and error alike.
    host.put_artifact(scratch_file.path(), task, ACCEPTANCE_ARTIFACT_PATH)?;
    Ok(AcceptOutcome {
        acceptance: candidate,
        recorded: true,
    })
}

/// After delivery lands: the task must be `review`/`done` (a failed or
/// non-terminal run leaves it elsewhere), then `validate_delivery` re-checks
/// the published commit on the bound workspace before anything is stored.
fn accept_output(input: Value, workspace_root: &Path, host: &dyn TaskHost) -> Value {
    let input: AcceptInput = match serde_json::from_value(input) {
        Ok(input) => input,
        Err(error) => return error_envelope("invalid_request", error.to_string()),
    };
    // Confirm the bound workspace is a real corpus before touching the host
    // at all, the same fail-fast order `link` uses: a conformance sandbox's
    // empty, non-Git workspace refuses here, deterministically, with no host
    // callback involved.
    if let Err(error) = Research::open(workspace_root) {
        return error_reply(&error);
    }
    let task = input.task_id.as_str();
    let state = match host.task_state(task) {
        Ok(state) => state,
        Err(error) => return error_reply(&error),
    };
    if state.status != "review" && state.status != "done" {
        return error_envelope(
            "refused",
            format!(
                "Task {task} is `{}`, not `review` or `done`; accept only runs after a successful, terminal delivery",
                state.status
            ),
        );
    }
    let Some(run) = state.job_run_id.as_deref() else {
        return error_envelope(
            "refused",
            format!("Task {task} has no recorded run to accept"),
        );
    };
    let report = match Research::open(workspace_root).and_then(|research| {
        research.validate_delivery(&Expected {
            research_id: Some(&input.research_id),
            task,
            run,
        })
    }) {
        Ok(report) => report,
        Err(error) => return error_reply(&error),
    };
    if !report.valid() {
        return error_envelope(
            report.findings[0].reason.as_str(),
            report
                .findings
                .iter()
                .map(|finding| format!("{}: {}", finding.reason.as_str(), finding.message))
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    match accept_record(workspace_root, &input.research_id, &report, run, task, host) {
        Ok(outcome) => json!({
            "ok": true,
            "output": {
                "research_id": outcome.acceptance.research_id,
                "task_id": task,
                "commit": outcome.acceptance.commit,
                "blob": outcome.acceptance.blob,
                "run_id": outcome.acceptance.run_id,
                "artifact_digests": outcome.acceptance.artifact_digests,
                "recorded": outcome.recorded,
            },
        }),
        Err(error) => error_reply(&error),
    }
}

fn handle(request_bytes: &[u8], host: &dyn TaskHost) -> Value {
    let request: Value = match serde_json::from_slice(request_bytes) {
        Ok(request) => request,
        Err(error) => {
            return error_envelope(
                "invalid_request",
                format!("Malformed plugin tool call: {error}"),
            );
        }
    };
    let Some(tool) = request.get("tool").and_then(Value::as_str) else {
        return error_envelope("invalid_request", "Missing plugin tool call `tool` name");
    };
    let input = request.get("input").cloned().unwrap_or(json!({}));
    if !input.is_object() {
        return error_envelope("invalid_request", "Plugin tool input must be a JSON object");
    }
    if verb(tool) == "version" {
        return match serde_json::from_value::<VersionInput>(input) {
            Ok(_) => version_output(),
            Err(error) => error_envelope("invalid_request", error.to_string()),
        };
    }
    if verb(tool) == "validate" {
        return validate_output(&request, input);
    }
    let workspace_root = request
        .pointer("/context/workspace_root")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let Some(workspace_root) = workspace_root else {
        return error_envelope("workspace_required", "This tool requires a bound workspace");
    };
    if verb(tool) == "link" {
        return link_output(&workspace_root, input, host);
    }
    if verb(tool) == "accept" {
        return accept_output(input, &workspace_root, host);
    }
    if panels::is_panel_verb(verb(tool)) {
        return panels::serve(verb(tool), input, &workspace_root, host);
    }
    let Some(operation) = operation_for_verb(verb(tool)) else {
        return error_envelope(
            "invalid_request",
            format!("Unknown research plugin tool: {tool}"),
        );
    };
    match Application::local(&workspace_root).and_then(|app| app.execute(operation, input)) {
        Ok(output) => json!({"ok": true, "output": output}),
        Err(error) => error_reply(&error),
    }
}

/// Serve exactly one plugin tool call: all of stdin is one request, all of
/// stdout is one reply. Only an I/O or encoding failure while handling the
/// process streams is a Rust `Err`; a bad request or a Core refusal is
/// reported as `ok:false` in the reply, never a nonzero exit.
pub fn serve_plugin_tool_call(reader: impl Read, writer: impl Write) -> Result<()> {
    serve_plugin_tool_call_with_host(reader, writer, &OrbitCliTaskHost)
}

pub(crate) fn serve_plugin_tool_call_with_host(
    reader: impl Read,
    mut writer: impl Write,
    host: &dyn TaskHost,
) -> Result<()> {
    let mut request_bytes = Vec::new();
    reader
        .take(MAX_PLUGIN_REQUEST_BYTES + 1)
        .read_to_end(&mut request_bytes)?;
    let response = if request_bytes.len() as u64 > MAX_PLUGIN_REQUEST_BYTES {
        error_envelope(
            "invalid_request",
            format!(
                "Plugin request exceeds {MAX_PLUGIN_REQUEST_BYTES} bytes; reduce the input or context"
            ),
        )
    } else {
        handle(&request_bytes, host)
    };
    serde_json::to_writer(&mut writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
