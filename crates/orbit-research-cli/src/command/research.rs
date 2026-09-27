//! Research command argument projection.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::parse::{ModeArg, PlanShape, ResearchOperation, WriterMode};
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
        ResearchOperation::WorkLinks { corpus } => Ok((corpus, Operation::WorkLinks, json!({}))),
        ResearchOperation::Create {
            corpus,
            kind,
            title,
            body,
            request_key,
            tags,
            derived_from,
            status,
            mode,
        } => {
            let mut input = json!({
                "kind": kind,
                "title": title,
                "body": body,
                "request_key": request_key,
                "tags": tags,
                "derived_from": derived_from,
            });
            if let Some(status) = status {
                input["status"] = json!(status);
            }
            Ok((corpus, Operation::Create, with_mode(input, &mode)))
        }
        ResearchOperation::Capture {
            corpus,
            text,
            tags,
            request_key,
            mode,
        } => {
            let mut input = json!({"text": text, "tags": tags});
            if let Some(key) = request_key {
                input["request_key"] = json!(key);
            }
            Ok((corpus, Operation::Capture, with_mode(input, &mode)))
        }
        ResearchOperation::Revise {
            corpus,
            id,
            expected_blob,
            title,
            body,
            body_file,
            tags,
            status,
            tests,
            orbit_task,
            orbit_run,
            manifest_file,
            mode,
        } => {
            let mut input = json!({"id": id, "expected_blob": expected_blob});
            let body = match body_file {
                Some(path) => Some(read(&path)?),
                None => body,
            };
            for (field, value) in [("title", title), ("body", body), ("status", status)] {
                if let Some(value) = value {
                    input[field] = json!(value);
                }
            }
            for (field, values) in [("tags", tags), ("tests", tests)] {
                if !values.is_empty() {
                    input[field] = json!(values);
                }
            }
            if orbit_task.is_some() || orbit_run.is_some() {
                input["orbit"] = json!({"task": orbit_task, "run": orbit_run});
            }
            if let Some(path) = manifest_file {
                input["manifest"] = serde_json::from_str(&read(&path)?)
                    .map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
            }
            Ok((corpus, Operation::Revise, with_mode(input, &mode)))
        }
        ResearchOperation::Assess {
            corpus,
            id,
            expected_blob,
            research,
            revision,
            verdict,
            strength,
            note,
            mode,
        } => {
            let mut input = json!({
                "id": id,
                "expected_blob": expected_blob,
                "research": research,
                "revision": revision,
                "verdict": verdict,
                "strength": strength,
            });
            if let Some(note) = note {
                input["note"] = json!(note);
            }
            Ok((corpus, Operation::Assess, with_mode(input, &mode)))
        }
        ResearchOperation::ReviseQuestion {
            corpus,
            id,
            expected_blob,
            title,
            body,
            tags,
            mode,
        } => Ok((
            corpus,
            Operation::ReviseQuestion,
            with_mode(
                json!({
                    "id": id,
                    "expected_blob": expected_blob,
                    "title": title,
                    "body": body,
                    "tags": tags,
                }),
                &mode,
            ),
        )),
        ResearchOperation::Plan {
            corpus,
            shape,
            research_id,
            objective,
            unit,
            contributions,
        } => {
            let input = match shape {
                PlanShape::Investigation => json!({
                    "shape": "investigation",
                    "research_id": research_id,
                    "objective": objective
                        .ok_or_else(|| "--objective is required for --shape investigation".to_owned())?,
                }),
                PlanShape::Contribution => json!({
                    "shape": "contribution",
                    "research_id": research_id,
                    "unit": unit
                        .ok_or_else(|| "--unit is required for --shape contribution".to_owned())?,
                    "objective": objective
                        .ok_or_else(|| "--objective is required for --shape contribution".to_owned())?,
                }),
                PlanShape::Synthesis => json!({
                    "shape": "synthesis",
                    "research_id": research_id,
                    "units": contributions,
                }),
            };
            Ok((corpus, Operation::Plan, input))
        }
    }
}

/// `auto` leaves the mode to detection; a named mode becomes an assertion.
fn with_mode(mut input: Value, mode: &ModeArg) -> Value {
    match mode.mode {
        WriterMode::Auto => (),
        WriterMode::Primary => input["mode"] = json!("primary"),
        WriterMode::Worktree => input["mode"] = json!("worktree"),
    }
    input
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))
}
