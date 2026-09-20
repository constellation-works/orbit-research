//! Shared application operations for CLI and MCP; dashboard calls the same Corpus methods.
use crate::{Error, Research as Corpus, Result, work::WorkPlan};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    request_key: String,
    kind: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    derived_from: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviseQuestion {
    id: String,
    expected_blob: String,
    title: String,
    body: String,
    tags: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Investigation {
    research_id: String,
    objective: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Work {
    research_id: String,
    unit: String,
    objective: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Synthesis {
    research_id: String,
    units: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkWork {
    request_key: String,
    title: String,
    crew: String,
    plan: WorkPlan,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestKey {
    request_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dispatch {
    request_key: String,
    base: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateResult {
    request_key: String,
    receipt_path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

// Stable facade for existing transport callers.
pub use crate::{config::BackendSettings, runtime::Application};

impl Application {
    pub fn call(&self, operation: &str, input: Value) -> Result<Value> {
        let corpus = &self.corpus;
        let backend =
            || {
                self.backend.as_ref().ok_or_else(|| {
                    Error::Invalid(
            "Orbit backend is not configured; local research operations remain available".into())
                })
            };
        match operation {
            "research.backend" => {
                let _: Empty = serde_json::from_value(input)?;
                backend()?.inspect()
            }
            "research.work_links" => {
                let _: Empty = serde_json::from_value(input)?;
                Ok(serde_json::to_value(corpus.work_links()?)?)
            }
            "research.link_work" => {
                let i: LinkWork = serde_json::from_value(input)?;
                Ok(serde_json::to_value(corpus.link_work(
                    backend()?,
                    &i.request_key,
                    &i.title,
                    &i.crew,
                    &i.plan,
                )?)?)
            }
            "research.dispatch" => {
                let i: Dispatch = serde_json::from_value(input)?;
                corpus.dispatch_work(backend()?, &i.request_key, &i.base)
            }
            "research.work_status" | "research.promote" | "research.cancel" => {
                let i: RequestKey = serde_json::from_value(input)?;
                match operation {
                    "research.work_status" => corpus.work_status(backend()?, &i.request_key),
                    "research.promote" => corpus.promote_work(backend()?, &i.request_key),
                    _ => corpus.cancel_work(backend()?, &i.request_key),
                }
            }
            "research.validate_result" => {
                let i: ValidateResult = serde_json::from_value(input)?;
                Ok(serde_json::to_value(corpus.validate_work_result(
                    backend()?,
                    &i.request_key,
                    &i.receipt_path,
                    &self.publication_ref,
                )?)?)
            }
            _ => call_local(corpus, operation, input),
        }
    }
}

pub fn call(root: &Path, operation: &str, input: Value) -> Result<Value> {
    Application::local(root)?.call(operation, input)
}
fn call_local(corpus: &Corpus, operation: &str, input: Value) -> Result<Value> {
    match operation {
        "research.list" | "research.check" => {
            let _: Empty = serde_json::from_value(input)?;
            serde_json::to_value(corpus.snapshot()?).map_err(Error::from)
        }
        "research.create" => {
            let i: Create = serde_json::from_value(input)?;
            serde_json::to_value(corpus.reserve(
                &i.request_key,
                &i.kind,
                &i.title,
                &i.body,
                i.tags,
                i.derived_from,
            )?)
            .map_err(Error::from)
        }
        "research.revise_question" => {
            let i: ReviseQuestion = serde_json::from_value(input)?;
            Ok(serde_json::to_value(corpus.revise_question(
                &i.id,
                &i.expected_blob,
                &i.title,
                &i.body,
                i.tags,
            )?)?)
        }
        "research.plan_investigation" => {
            let i: Investigation = serde_json::from_value(input)?;
            serde_json::to_value(corpus.investigation(&i.research_id, &i.objective)?)
                .map_err(Error::from)
        }
        "research.plan_contribution" => {
            let i: Work = serde_json::from_value(input)?;
            serde_json::to_value(corpus.contribution(&i.research_id, &i.unit, &i.objective)?)
                .map_err(Error::from)
        }
        "research.plan_synthesis" => {
            let i: Synthesis = serde_json::from_value(input)?;
            serde_json::to_value(corpus.synthesis(&i.research_id, &i.units)?).map_err(Error::from)
        }
        _ => Err(Error::Invalid(format!(
            "Unknown research operation: {operation}"
        ))),
    }
}
pub fn tools() -> Value {
    let text = json!({"type":"string","minLength":1});
    let list = json!({"type":"array","items":{"type":"string"}});
    json!([
        {"name":"research.backend","description":"Inspect the configured Orbit backend and compatibility. Never changes backend scope.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"research.work_links","description":"List local request correlations pointing to authoritative Orbit tasks. Cached pointers are not fresh run status.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"research.work_status","description":"Read fresh Orbit task/run evidence for a linked request.","inputSchema":{"type":"object","required":["request_key"],"properties":{"request_key":text},"additionalProperties":false}},
        {"name":"research.promote","description":"Explicitly approve the linked task for execution. Requires configured backend authority; does not dispatch.","inputSchema":{"type":"object","required":["request_key"],"properties":{"request_key":text},"additionalProperties":false}},
        {"name":"research.dispatch","description":"Explicitly dispatch an approved linked task. Unknown prior submissions are reconciled, never blindly repeated.","inputSchema":{"type":"object","required":["request_key","base"],"properties":{"request_key":text,"base":text},"additionalProperties":false}},
        {"name":"research.cancel","description":"Explicitly cancel only the run currently correlated with the linked task.","inputSchema":{"type":"object","required":["request_key"],"properties":{"request_key":text},"additionalProperties":false}},
        {"name":"research.validate_result","description":"Fetch an Orbit receipt and verify its task/run, published record and artifact identities. Does not infer scientific support.","inputSchema":{"type":"object","required":["request_key","receipt_path"],"properties":{"request_key":text,"receipt_path":text},"additionalProperties":false}},
        {"name":"research.link_work","description":"Create an Orbit task from a validated work plan. Persist request_key for reconciliation. Does not dispatch.","inputSchema":{"type":"object","required":["request_key","title","crew","plan"],"properties":{"request_key":text,"title":text,"crew":text,"plan":{"type":"object","additionalProperties":false,"required":["research_id","corpus_revision","research_blob","mode","context_files","instructions"],"properties":{"research_id":text,"corpus_revision":text,"research_blob":text,"mode":{"enum":["investigation","contribution","synthesis"]},"context_files":list,"instructions":text}}},"additionalProperties":false}},
        {"name":"research.list","description":"Read the validated canonical research corpus and tags.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"research.create","description":"Reserve and commit a canonical Q/H/T/R record on the integration checkout. Retain request_key across retries; never allocate IDs in worker worktrees. Does not dispatch any agent.","inputSchema":{"type":"object","required":["request_key","kind","title"],"properties":{"request_key":text,"kind":{"enum":["Q","H","T","R"]},"title":text,"body":{"type":"string"},"tags":list,"derived_from":list},"additionalProperties":false}},
        {"name":"research.revise_question","description":"Commit a question revision only if expected_blob still matches. Frozen path and lineage are preserved; requires clean primary checkout.","inputSchema":{"type":"object","required":["id","expected_blob","title","body","tags"],"properties":{"id":text,"expected_blob":text,"title":text,"body":{"type":"string"},"tags":list},"additionalProperties":false}},
        {"name":"research.plan_investigation","description":"Plan a single task owning one reserved research item, including its canonical result and evidence. Does not dispatch.","inputSchema":{"type":"object","required":["research_id","objective"],"properties":{"research_id":text,"objective":text},"additionalProperties":false}},
        {"name":"research.plan_contribution","description":"Plan disjoint code and artifact paths for one contribution to an existing research item. Use returned context_files on the Orbit task; shared summary is read-only.","inputSchema":{"type":"object","required":["research_id","unit","objective"],"properties":{"research_id":text,"unit":text,"objective":text},"additionalProperties":false}},
        {"name":"research.plan_synthesis","description":"Plan a follow-up Orbit task to reconcile contributions into the shared research README and input manifest. Schedule after contributing tasks deliver.","inputSchema":{"type":"object","required":["research_id","units"],"properties":{"research_id":text,"units":list},"additionalProperties":false}}
    ])
}
