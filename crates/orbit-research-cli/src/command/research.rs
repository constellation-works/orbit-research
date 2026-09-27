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
        ResearchOperation::Show { corpus, id } => Ok((corpus, Operation::Show, json!({"id": id}))),
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
