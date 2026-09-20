//! Shared application entry points. Protocol strings are parsed only at the edge.
use super::{Operation, request::*};
use crate::{Error, Result, adapter::orbit::OrbitBackend};
use serde_json::Value;
use std::path::Path;

pub use crate::{BackendSettings, runtime::Application};

impl Application {
    /// Decode an external protocol name, then enter typed dispatch.
    pub fn call(&self, name: &str, input: Value) -> Result<Value> {
        self.execute(name.parse()?, input)
    }

    /// Internal callers select a known operation without stringly typed routing.
    pub fn execute(&self, operation: Operation, input: Value) -> Result<Value> {
        operation.execute(self, input)
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

fn configured_backend(app: &Application) -> Result<&OrbitBackend> {
    app.orbit.as_ref().ok_or_else(|| {
        Error::Invalid(
            "Orbit backend is not configured; local research operations remain available".into(),
        )
    })
}

pub(super) fn backend(app: &Application, _: Empty) -> Result<Value> {
    configured_backend(app)?.inspect()
}

pub(super) fn work_links(app: &Application, _: Empty) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.work_links()?)?)
}

pub(super) fn work_status(app: &Application, input: RequestKey) -> Result<Value> {
    app.corpus
        .work_status(configured_backend(app)?, &input.request_key)
}

pub(super) fn promote(app: &Application, input: RequestKey) -> Result<Value> {
    app.corpus
        .promote_work(configured_backend(app)?, &input.request_key)
}

pub(super) fn cancel(app: &Application, input: RequestKey) -> Result<Value> {
    app.corpus
        .cancel_work(configured_backend(app)?, &input.request_key)
}

pub(super) fn dispatch(app: &Application, input: Dispatch) -> Result<Value> {
    app.corpus
        .dispatch_work(configured_backend(app)?, &input.request_key, &input.base)
}

pub(super) fn link_work(app: &Application, input: LinkWork) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.link_work(
        configured_backend(app)?,
        &input.request_key,
        &input.title,
        &input.crew,
        &input.plan,
    )?)?)
}

pub(super) fn validate_result(app: &Application, input: ValidateResult) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.validate_work_result(
        configured_backend(app)?,
        &input.request_key,
        &input.receipt_path,
        &app.publication_ref,
    )?)?)
}

pub(super) fn list(app: &Application, _: Empty) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.snapshot()?)?)
}

pub(super) fn create(app: &Application, input: Create) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.reserve(
        &input.request_key,
        input.kind.as_str(),
        &input.title,
        &input.body,
        input.tags,
        input.derived_from,
    )?)?)
}

pub(super) fn revise_question(app: &Application, input: ReviseQuestion) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.revise_question(
        &input.id,
        &input.expected_blob,
        &input.title,
        &input.body,
        input.tags,
    )?)?)
}

pub(super) fn investigation(app: &Application, input: Investigation) -> Result<Value> {
    Ok(serde_json::to_value(
        app.corpus
            .investigation(&input.research_id, &input.objective)?,
    )?)
}

pub(super) fn contribution(app: &Application, input: Contribution) -> Result<Value> {
    Ok(serde_json::to_value(app.corpus.contribution(
        &input.research_id,
        &input.unit,
        &input.objective,
    )?)?)
}

pub(super) fn synthesis(app: &Application, input: Synthesis) -> Result<Value> {
    Ok(serde_json::to_value(
        app.corpus.synthesis(&input.research_id, &input.units)?,
    )?)
}
