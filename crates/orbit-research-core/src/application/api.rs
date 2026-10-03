//! Shared application entry points. Protocol strings are parsed only at the edge.
use super::{Operation, request::*};
use crate::{Error, Result};
use orbit_research_store::{
    edit::{self, Assessment, Edit},
    writer::{WriteMode, WriteOutcome},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub use crate::runtime::Application;

impl Application {
    /// Decode an external protocol name, then enter typed dispatch.
    pub fn call(&self, name: &str, input: Value) -> Result<Value> {
        self.execute(name.parse()?, input)
    }

    /// Internal callers select a known operation without stringly typed routing.
    pub fn execute(&self, operation: Operation, input: Value) -> Result<Value> {
        operation.execute(self, input)
    }

    /// Record (or recall) intent to link a reserved research item to an
    /// Orbit task, bound to the task scope `context_files` (the research
    /// directory when `None`; otherwise a scope `plan` derives for the item).
    /// Not an `Operation`: only a transport that can reach Orbit (the plugin's
    /// `link` tool) calls this, never Core itself.
    pub fn link_intent(
        &self,
        request_key: &str,
        research_id: &str,
        context_files: Option<&[String]>,
    ) -> Result<super::operations::LinkPreparation> {
        self.corpus
            .link_intent(request_key, research_id, context_files)
    }

    /// Record the Orbit task adopted or created for a prior `link_intent`.
    pub fn link_confirm(
        &self,
        request_key: &str,
        task_id: &str,
    ) -> Result<super::operations::Link> {
        self.corpus.link_confirm(request_key, task_id)
    }
}

pub fn call(root: &Path, name: &str, input: Value) -> Result<Value> {
    Application::local(root)?.call(name, input)
}

pub fn tools() -> Value {
    Value::Array(
        Operation::ALL
            .iter()
            .map(|operation| {
                // The descriptor contains only serializable literals and JSON schemas.
                serde_json::json!(operation.definition())
            })
            .collect(),
    )
}

pub(super) fn work_links(app: &Application, _: Empty) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.work_links()?)?)
}

pub(super) fn list(app: &Application, _: Empty) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.snapshot()?)?)
}

pub(super) fn show(app: &Application, input: Show) -> Result<Value> {
    let snapshot = app.corpus.snapshot()?;
    let record = snapshot
        .records
        .into_iter()
        .find(|record| record.id == input.id)
        .ok_or_else(|| Error::NotFound(format!("Unknown research record id: {}", input.id)))?;
    Ok(serde_json::to_value(record)?)
}

#[derive(Serialize)]
struct ValidationSummary {
    valid: bool,
    base_revision: String,
    record_count: usize,
    tag_count: usize,
}

pub(super) fn check(app: &Application, _: Empty) -> Result<Value> {
    let snapshot = app.corpus.snapshot()?;
    Ok(serde_json::to_value(ValidationSummary {
        valid: true,
        base_revision: snapshot.revision,
        record_count: snapshot.records.len(),
        tag_count: snapshot.tags.len(),
    })?)
}

/// Refuse when the caller expected the other writer mode.
fn require_mode(app: &Application, expected: Option<Mode>) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let expected = match expected {
        Mode::Primary => WriteMode::Primary,
        Mode::Worktree => WriteMode::Worktree,
    };
    let detected = app.corpus.store.write_mode()?;
    if detected != expected {
        return Err(Error::Refused(format!(
            "Expected {expected} mode, but this checkout writes in {detected} mode"
        )));
    }
    Ok(())
}

pub(super) fn create(app: &Application, input: Create) -> Result<Value> {
    require_mode(app, input.mode)?;
    let kind = input.kind.as_str();
    if let Some(status) = &input.status {
        let initial = orbit_research_store::initial_status(kind);
        if status != initial {
            return Err(Error::InvalidInput(format!(
                "New {kind} records start as {initial}"
            )));
        }
    }
    let reservation = app.corpus.reserve(
        &input.request_key,
        kind,
        &input.title,
        &input.body,
        input.tags,
        input.derived_from,
    )?;
    Ok(serde_json::to_value(WriteOutcome::Primary(reservation))?)
}

pub(super) fn capture(app: &Application, input: Capture) -> Result<Value> {
    require_mode(app, input.mode)?;
    let text = input.text.trim();
    let title = text
        .lines()
        .map(|line| line.trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .ok_or_else(|| Error::InvalidInput("Capture text is required".into()))?;
    let key = match input.request_key {
        Some(key) => key,
        None => format!(
            "capture-{:x}",
            Sha256::digest(serde_json::to_vec(&json!([text, input.tags]))?)
        ),
    };
    let reservation = app
        .corpus
        .reserve(&key, "Q", title, text, input.tags, vec![])?;
    Ok(serde_json::to_value(WriteOutcome::Primary(reservation))?)
}

pub(super) fn revise_question(app: &Application, input: ReviseQuestion) -> Result<Value> {
    require_mode(app, input.mode)?;
    if input.title.is_none() && input.body.is_none() && input.tags.is_none() {
        return Err(Error::InvalidInput(
            "Nothing to change: give at least one of title, body or tags".into(),
        ));
    }
    let reservation = app.corpus.revise_question(
        &input.id,
        &input.expected_blob,
        input.title.as_deref(),
        input.body.as_deref(),
        input.tags,
    )?;
    Ok(serde_json::to_value(WriteOutcome::Primary(reservation))?)
}

pub(super) fn revise(app: &Application, input: Revise) -> Result<Value> {
    require_mode(app, input.mode)?;
    let revision = Edit {
        title: input.title,
        body: input.body,
        tags: input.tags,
        status: input.status,
        tests: input.tests,
        orbit: input.orbit.map(|orbit| edit::OrbitLink {
            task: orbit.task,
            run: orbit.run,
        }),
        manifest: input.manifest,
    };
    if revision.is_empty() {
        return Err(Error::InvalidInput(
            "Nothing to change: give at least one of title, body, tags, status, tests, orbit or manifest".into(),
        ));
    }
    Ok(serde_json::to_value(app.corpus.store.revise(
        &input.id,
        &input.expected_blob,
        &revision,
    )?)?)
}

/// The verdict is the caller's explicit judgement. Acceptance proves only that
/// the result was delivered and validated; it never supplies or strengthens one.
pub(super) fn assess(app: &Application, input: Assess) -> Result<Value> {
    require_mode(app, input.mode)?;
    let assessment = Assessment {
        research: input.research,
        revision: input.revision,
        verdict: input.verdict.as_str().into(),
        strength: input.strength.as_str().into(),
        note: input.note,
    };
    let reservation =
        app.corpus
            .store
            .assess(&input.id, &input.expected_blob, &assessment, |snapshot| {
                super::acceptance::require_acceptance(
                    app.acceptance.as_ref(),
                    snapshot,
                    &assessment.research,
                )
            })?;
    Ok(serde_json::to_value(WriteOutcome::Primary(reservation))?)
}

pub(super) fn plan(app: &Application, input: Plan) -> Result<Value> {
    use crate::work::PlanShape;
    let (research_id, shape) = match input {
        Plan::Investigation {
            research_id,
            objective,
        } => (research_id, PlanShape::Investigation { objective }),
        Plan::Contribution {
            research_id,
            unit,
            objective,
        } => (research_id, PlanShape::Contribution { unit, objective }),
        Plan::Synthesis { research_id, units } => (research_id, PlanShape::Synthesis { units }),
    };
    Ok(serde_json::to_value(app.corpus.plan(&research_id, shape)?)?)
}
