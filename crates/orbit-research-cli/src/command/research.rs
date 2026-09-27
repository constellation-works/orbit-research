//! Research command argument projection.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::parse::ResearchOperation;
use orbit_research_core::application::Operation;

/// Convert parsed CLI arguments into the Core operation boundary.
///
/// This keeps file input and operation naming in the command layer while Core
/// remains responsible for validation, persistence, and backend authority.
pub(crate) fn prepare(operation: ResearchOperation) -> Result<(PathBuf, Operation, Value), String> {
    match operation {
        ResearchOperation::Show { .. } => {
            Err("Record detail uses the read-only show handler".into())
        }
        ResearchOperation::List { corpus } => Ok((corpus, Operation::List, json!({}))),
        ResearchOperation::Check { corpus } => Ok((corpus, Operation::Check, json!({}))),
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
            Operation::Create,
            json!({
                "kind": kind,
                "title": title,
                "body": body,
                "request_key": request_key,
                "tags": tags,
                "derived_from": derived_from,
            }),
        )),
        ResearchOperation::PlanContribution {
            corpus,
            research_id,
            unit,
            objective,
        } => Ok((
            corpus,
            Operation::PlanContribution,
            json!({"research_id":research_id,"unit":unit,"objective":objective}),
        )),
        ResearchOperation::PlanInvestigation {
            corpus,
            research_id,
            objective,
        } => Ok((
            corpus,
            Operation::PlanInvestigation,
            json!({"research_id":research_id,"objective":objective}),
        )),
        ResearchOperation::PlanSynthesis {
            corpus,
            research_id,
            units,
        } => Ok((
            corpus,
            Operation::PlanSynthesis,
            json!({"research_id":research_id,"units":units}),
        )),
    }
}

/// Select a detail view from Core's validated snapshot; no alternate reader.
pub(crate) fn show(
    application: &orbit_research_core::Application,
    id: &str,
) -> Result<Value, String> {
    let snapshot = application
        .execute(Operation::List, json!({}))
        .map_err(|error| error.to_string())?;
    snapshot["records"]
        .as_array()
        .and_then(|records| records.iter().find(|record| record["id"] == id))
        .cloned()
        .ok_or_else(|| format!("Research record {id} was not found"))
}
