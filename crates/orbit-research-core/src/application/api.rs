//! Shared application entry points. Protocol strings are parsed only at the edge.
use super::{Operation, request::*};
use crate::Result;
use serde::Serialize;
use serde_json::Value;
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
