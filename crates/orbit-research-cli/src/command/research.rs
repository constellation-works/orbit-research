//! Research command argument projection.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::parse::ResearchOperation;

/// Convert parsed CLI arguments into the Core operation boundary.
///
/// This keeps file input and operation naming in the command layer while Core
/// remains responsible for validation, persistence, and backend authority.
pub(crate) fn prepare(
    operation: ResearchOperation,
) -> Result<(PathBuf, &'static str, Value), String> {
    match operation {
        ResearchOperation::Backend { corpus } => Ok((corpus, "research.backend", json!({}))),
        ResearchOperation::Status {
            corpus,
            request_key,
        } => Ok((
            corpus,
            "research.work_status",
            json!({"request_key": request_key}),
        )),
        ResearchOperation::Link {
            corpus,
            plan,
            request_key,
            title,
            crew,
        } => Ok((
            corpus,
            "research.link_work",
            json!({"request_key":request_key,"title":title,"crew":crew,"plan":read_json(&plan)?}),
        )),
        ResearchOperation::Promote {
            corpus,
            request_key,
        } => Ok((
            corpus,
            "research.promote",
            json!({"request_key": request_key}),
        )),
        ResearchOperation::Dispatch {
            corpus,
            request_key,
            base,
        } => Ok((
            corpus,
            "research.dispatch",
            json!({"request_key":request_key,"base":base}),
        )),
        ResearchOperation::Cancel {
            corpus,
            request_key,
        } => Ok((
            corpus,
            "research.cancel",
            json!({"request_key": request_key}),
        )),
        ResearchOperation::ValidateResult {
            corpus,
            request_key,
            receipt_path,
        } => Ok((
            corpus,
            "research.validate_result",
            json!({"request_key":request_key,"receipt_path":receipt_path}),
        )),
        ResearchOperation::List { corpus } => Ok((corpus, "research.list", json!({}))),
        ResearchOperation::Check { corpus } => Ok((corpus, "research.check", json!({}))),
        ResearchOperation::Create {
            corpus,
            kind,
            title,
            body,
            request_key,
            tags,
            derived_from,
        } => Ok((
            corpus,
            "research.create",
            json!({"kind":kind,"title":title,"body":body,"request_key":request_key,"tags":tags,"derived_from":derived_from}),
        )),
        ResearchOperation::PlanContribution {
            corpus,
            research_id,
            unit,
            objective,
        } => Ok((
            corpus,
            "research.plan_contribution",
            json!({"research_id":research_id,"unit":unit,"objective":objective}),
        )),
        ResearchOperation::PlanInvestigation {
            corpus,
            research_id,
            objective,
        } => Ok((
            corpus,
            "research.plan_investigation",
            json!({"research_id":research_id,"objective":objective}),
        )),
        ResearchOperation::PlanSynthesis {
            corpus,
            research_id,
            units,
        } => Ok((
            corpus,
            "research.plan_synthesis",
            json!({"research_id":research_id,"units":units}),
        )),
    }
}

fn read_json(path: &std::path::Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}
