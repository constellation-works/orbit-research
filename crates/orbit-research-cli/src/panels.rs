//! Dashboard panel sources for the Orbit plugin: four read-only tools whose
//! output Orbit's generic panel renderer draws without plugin code.
//!
//! The renderer fixes what a good output looks like, so the shapes here are
//! deliberate:
//!
//! - A `table` panel draws one column per key, keys taken from the rows and
//!   printed verbatim as upper-case headers. A `kv` panel prints each key as
//!   its label. Orbit parses this reply into a `serde_json::Value` without
//!   `preserve_order`, so keys reach the dashboard in alphabetical order no
//!   matter how they are emitted. Every key below is therefore chosen so that
//!   alphabetical order is also the order a reader wants.
//! - Cells are flat scalars. A nested object would render as a JSON blob, so
//!   lists become comma-joined text, a missing value is `null` (drawn as a
//!   dash), dates are `YYYY-MM-DD` and long titles are cut with an ellipsis.
//! - An empty array renders only as a generic "No data.", so an empty result
//!   is one row `{"status": "<what is true and what to do next>"}`. A
//!   workspace without a usable corpus gets the same kind of row (or, for the
//!   `kv` panel, a `Corpus`/`Detail` pair) rather than an error, because the
//!   renderer would show a failed tool call as a raw error string.
//!
//! None of these tools writes. `awaiting-acceptance` additionally reads task
//! artifacts through the same `orbit.task.show`/`orbit.task.artifact.get`
//! callbacks `accept` uses; a callback that fails leaves that row's status
//! `acceptance unknown` instead of guessing.
use crate::acceptance_cache;
use crate::plugin::{ACCEPTANCE_ARTIFACT_PATH, TaskHost, error_envelope};
use orbit_research_core::{Error, Record, Research, Result, Snapshot};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// The tool verbs this module serves, in manifest order.
pub(crate) const VERBS: [&str; 4] = [
    "open-questions",
    "awaiting-acceptance",
    "hypotheses",
    "corpus-health",
];

/// Longest title shown in a cell before it is cut with an ellipsis.
const TITLE_WIDTH: usize = 80;
/// Longest corpus problem shown in the unavailable state.
const PROBLEM_WIDTH: usize = 160;
/// How long `awaiting-acceptance` may spend on Orbit callbacks. The backend's
/// own timeout is 30 s; stopping earlier lets the panel still answer.
const CALLBACK_BUDGET: Duration = Duration::from_secs(20);
/// Consecutive callback failures, with no success yet, after which the
/// remaining rows are reported unknown without more attempts.
const CALLBACK_GIVE_UP: usize = 3;

const NO_CORPUS: &str = "This workspace has no research corpus yet. \
     Run `orbit-research workspace init <path>` to create one.";

// The panel tools take no input; an unknown field is a mistake, not ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelInput {}

pub(crate) fn is_panel_verb(verb: &str) -> bool {
    VERBS.contains(&verb)
}

/// Serve one panel tool call and return the complete plugin reply.
pub(crate) fn serve(verb: &str, input: Value, workspace_root: &Path, host: &dyn TaskHost) -> Value {
    if let Err(error) = serde_json::from_value::<PanelInput>(input) {
        return error_envelope("invalid_request", error.to_string());
    }
    let (shape, built) = match verb {
        "open-questions" => (Shape::Table, build(workspace_root, open_questions)),
        "hypotheses" => (Shape::Table, build(workspace_root, hypotheses)),
        "corpus-health" => (Shape::Kv, build(workspace_root, corpus_health)),
        _ => (
            Shape::Table,
            build(workspace_root, |snapshot| {
                awaiting_acceptance(snapshot, host, workspace_root)
            }),
        ),
    };
    let output = built.unwrap_or_else(|error| unavailable(shape, workspace_root, &error));
    json!({"ok": true, "output": output})
}

#[derive(Clone, Copy)]
enum Shape {
    Table,
    Kv,
}

/// Open the corpus and derive one view. Every panel reads the working tree,
/// the same view `research check`, `list` and `show` read, so a panel that says
/// the corpus is unreadable and a `check` that says it is valid cannot disagree.
/// (`awaiting-acceptance` once read HEAD; in the primary checkout the two
/// differ only while someone has uncommitted edits, which writers refuse anyway.)
fn build(root: &Path, view: impl FnOnce(&Snapshot) -> Value) -> Result<Value> {
    let research = Research::open(root)?;
    Ok(view(&research.snapshot()?))
}

/// What a panel shows when the corpus cannot be read: where it is, the first
/// problem in a short sentence, and the exact command that lists them all.
/// Never the raw error. A workspace with no schema file is the common, benign case.
fn unavailable(shape: Shape, root: &Path, error: &Error) -> Value {
    let has_schema = std::fs::symlink_metadata(root.join("_scripts/schema.json")).is_ok();
    let (label, message) = if has_schema {
        let (first, more) = match error {
            Error::Corpus(issues) => (
                issues.first().map(ToString::to_string).unwrap_or_default(),
                issues.len().saturating_sub(1),
            ),
            other => (
                other
                    .to_string()
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                0,
            ),
        };
        let more = match more {
            0 => String::new(),
            1 => " 1 more problem not shown.".to_owned(),
            count => format!(" {count} more problems not shown."),
        };
        (
            "Invalid",
            format!(
                "The research corpus at {path} cannot be read: {}{more} Run `orbit-research research check --corpus {path}` for the full list.",
                sentence(&truncate_words(&first, PROBLEM_WIDTH)),
                path = root.display()
            ),
        )
    } else {
        ("No corpus", NO_CORPUS.to_owned())
    };
    match shape {
        Shape::Table => status_rows(message),
        Shape::Kv => json!({"Corpus": label, "Detail": message}),
    }
}

/// The one-row table that stands in for an empty or unavailable result.
fn status_rows(message: impl Into<String>) -> Value {
    json!([{"status": message.into()}])
}

fn sentence(text: &str) -> String {
    let text = text.trim_end();
    if text.ends_with(['.', '!', '?', '…']) {
        text.to_owned()
    } else {
        format!("{text}.")
    }
}

// ---- Cell helpers ---------------------------------------------------------

fn text<'a>(record: &'a Record, key: &str) -> Option<&'a str> {
    record
        .metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn ids<'a>(record: &'a Record, key: &str) -> Vec<&'a str> {
    record
        .metadata
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn orbit_field<'a>(record: &'a Record, field: &str) -> Option<&'a str> {
    record
        .metadata
        .pointer(&format!("/orbit/{field}"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// One line of at most `max` characters; longer text ends in an ellipsis.
fn truncate(value: &str, max: usize) -> String {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let kept: String = line.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// One line of at most `max` characters, cut at a word boundary and ended with
/// an ellipsis, so a long problem never stops in the middle of a word or value.
fn truncate_words(value: &str, max: usize) -> String {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let head: String = line.chars().take(max.saturating_sub(1)).collect();
    let cut = head.rfind(' ').map_or(head.as_str(), |at| &head[..at]);
    format!("{}…", cut.trim_end_matches([' ', ',', ';', ':']))
}

fn title_cell(record: &Record) -> Value {
    json!(truncate(
        text(record, "title").unwrap_or(&record.id),
        TITLE_WIDTH
    ))
}

fn date_cell(value: Option<&str>) -> Value {
    match value {
        Some(value) => json!(value.get(..10).unwrap_or(value)),
        None => Value::Null,
    }
}

fn joined_cell<'a>(values: impl IntoIterator<Item = &'a str>) -> Value {
    let values: BTreeSet<&str> = values.into_iter().collect();
    if values.is_empty() {
        Value::Null
    } else {
        json!(values.into_iter().collect::<Vec<_>>().join(", "))
    }
}

fn records_of<'a>(snapshot: &'a Snapshot, kind: &'a str) -> impl Iterator<Item = &'a Record> {
    snapshot
        .records
        .iter()
        .filter(move |record| record.kind == kind)
}

// ---- Open questions -------------------------------------------------------

/// Keys sort as: id, question, tags, tasks, updated. Newest update first,
/// ties by id descending, so recent captures lead and the order is stable.
fn open_questions(snapshot: &Snapshot) -> Value {
    let total = records_of(snapshot, "Q").count();
    let mut open: Vec<&Record> = records_of(snapshot, "Q")
        .filter(|question| text(question, "status") == Some("open"))
        .collect();
    if open.is_empty() {
        return status_rows(match total {
            0 => "No questions captured yet. Capture one with `orbit-research research capture`."
                .to_owned(),
            1 => "No open questions. The only question is answered or dropped.".to_owned(),
            _ => format!("No open questions. All {total} questions are answered or dropped."),
        });
    }
    open.sort_by(|a, b| {
        updated_key(b)
            .cmp(&updated_key(a))
            .then_with(|| b.id.cmp(&a.id))
    });
    Value::Array(
        open.into_iter()
            .map(|question| {
                json!({
                    "id": question.id,
                    "question": title_cell(question),
                    "tags": joined_cell(ids(question, "tags")),
                    "tasks": tasks_cell(linked_tasks(snapshot, question)),
                    "updated": date_cell(text(question, "updated")),
                })
            })
            .collect(),
    )
}

/// The `YYYY-MM-DD` of a record's `updated`, which sorts as a date; a missing
/// date is the oldest.
fn updated_key(record: &Record) -> Option<&str> {
    text(record, "updated").map(|value| value.get(..10).unwrap_or(value))
}

/// How many task ids a cell lists before the rest are summarized.
const TASKS_SHOWN: usize = 3;

/// Distinct task ids in numeric order (`ORB-9` before `ORB-10`), at most
/// [`TASKS_SHOWN`] of them followed by `+N more`.
fn tasks_cell(tasks: Vec<&str>) -> Value {
    let tasks: BTreeSet<&str> = tasks.into_iter().collect();
    if tasks.is_empty() {
        return Value::Null;
    }
    let mut tasks: Vec<&str> = tasks.into_iter().collect();
    tasks.sort_by_key(|task| numeric_key(task));
    let hidden = tasks.len().saturating_sub(TASKS_SHOWN);
    let mut shown: Vec<String> = tasks
        .into_iter()
        .take(TASKS_SHOWN)
        .map(str::to_owned)
        .collect();
    if hidden > 0 {
        shown.push(format!("+{hidden} more"));
    }
    json!(shown.join(", "))
}

/// Orders `PREFIX-<number>` ids by prefix, then by the number's value; text
/// without trailing digits sorts by its text alone.
fn numeric_key(id: &str) -> (&str, u64, &str) {
    let digits = id.bytes().rev().take_while(u8::is_ascii_digit).count();
    let (prefix, number) = id.split_at(id.len() - digits);
    (prefix, number.parse().unwrap_or(0), id)
}

/// Orbit tasks recorded on research items that work on this question: items
/// derived from it, items that answer it, and items testing a hypothesis
/// derived from it. Only the item's own `orbit.task` frontmatter counts, so a
/// reserved item whose investigation has not yet written one has no task here.
fn linked_tasks<'a>(snapshot: &'a Snapshot, question: &'a Record) -> Vec<&'a str> {
    let hypotheses: BTreeSet<&str> = records_of(snapshot, "H")
        .filter(|hypothesis| ids(hypothesis, "derived_from").contains(&question.id.as_str()))
        .map(|hypothesis| hypothesis.id.as_str())
        .collect();
    let answers = ids(question, "answered_by");
    records_of(snapshot, "R")
        .filter(|item| {
            ids(item, "derived_from").contains(&question.id.as_str())
                || answers.contains(&item.id.as_str())
                || ids(item, "tests")
                    .iter()
                    .any(|tested| hypotheses.contains(tested))
        })
        .filter_map(|item| orbit_field(item, "task"))
        .collect()
}

// ---- Results awaiting acceptance ------------------------------------------

/// Keys sort as: id, reason, result, status, task, updated. `reason` is set
/// only for `acceptance unknown`, saying in a few words why Orbit could not be
/// asked or did not answer.
///
/// A result counts as accepted only when its task's artifact names this
/// research id and this exact README blob at HEAD, the same bar `assess`
/// applies. Positive answers are remembered per (id, task, blob) by
/// [`acceptance_cache`], which can only ever hide a row, never invent one.
fn awaiting_acceptance(snapshot: &Snapshot, host: &dyn TaskHost, workspace: &Path) -> Value {
    // Delivered: committed with both an Orbit task and run recorded, which is
    // what a finished `research_investigation` run writes.
    let delivered: Vec<(&Record, &str)> = records_of(snapshot, "R")
        .filter(|item| orbit_field(item, "run").is_some())
        .filter_map(|item| orbit_field(item, "task").map(|task| (item, task)))
        .collect();
    if delivered.is_empty() {
        return status_rows(
            "No delivered results yet. A result appears here once its investigation run commits it.",
        );
    }
    let started = Instant::now();
    let (mut failures, mut successes) = (0, 0);
    let mut rows = Vec::new();
    for (item, task) in &delivered {
        if acceptance_cache::is_accepted(workspace, &item.id, task, &item.git_blob) {
            continue;
        }
        let giving_up =
            (failures >= CALLBACK_GIVE_UP && successes == 0) || started.elapsed() > CALLBACK_BUDGET;
        let (status, reason) = if giving_up {
            let reason = if started.elapsed() > CALLBACK_BUDGET {
                "time budget exceeded"
            } else {
                "earlier tasks unreadable"
            };
            (Some("acceptance unknown"), Some(reason))
        } else {
            match host.get_artifact(task, ACCEPTANCE_ARTIFACT_PATH) {
                Ok(None) => {
                    successes += 1;
                    (Some("awaiting acceptance"), None)
                }
                Ok(Some(artifact)) => {
                    successes += 1;
                    let field = |name: &str| artifact.get(name).and_then(Value::as_str);
                    if field("research_id") != Some(&item.id) {
                        (Some("acceptance names another result"), None)
                    } else if field("blob") != Some(&item.git_blob) {
                        (Some("result changed since accepted"), None)
                    } else {
                        acceptance_cache::record(workspace, &item.id, task, &item.git_blob);
                        (None, None)
                    }
                }
                Err(_) => {
                    failures += 1;
                    (Some("acceptance unknown"), Some("task unreadable"))
                }
            }
        };
        if let Some(status) = status {
            rows.push(json!({
                "id": item.id,
                "reason": reason,
                "result": title_cell(item),
                "status": status,
                "task": task,
                "updated": date_cell(text(item, "updated")),
            }));
        }
    }
    if rows.is_empty() {
        return status_rows(match delivered.len() {
            1 => "Nothing awaiting acceptance. The only delivered result is accepted.".to_owned(),
            count => {
                format!("Nothing awaiting acceptance. All {count} delivered results are accepted.")
            }
        });
    }
    Value::Array(rows)
}

// ---- Hypotheses and assessments -------------------------------------------

/// Keys sort as: id, name, research, revision, status, verdict, when. That
/// reads as: which hypothesis, which result judged it, on which revision, with
/// what standing and verdict, and when.
///
/// One row per (hypothesis, revision, research result): the latest assessment
/// that result made on that revision. Results that disagree on a revision stay
/// on their own rows and the revision is marked disputed; nothing is averaged
/// or collapsed into one verdict. Earlier revisions keep their rows, marked
/// superseded, because assessments stay on the revision they judged.
fn hypotheses(snapshot: &Snapshot) -> Value {
    let mut rows = Vec::new();
    for hypothesis in records_of(snapshot, "H") {
        let current = hypothesis
            .metadata
            .get("revision")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        // Later array entries are later assessments, so a plain insert keeps
        // each result's latest verdict per revision.
        let mut latest: BTreeMap<u64, BTreeMap<&str, &Value>> = BTreeMap::new();
        for assessment in hypothesis
            .metadata
            .get("assessments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(revision), Some(research)) = (
                assessment.get("revision").and_then(Value::as_u64),
                assessment.get("research").and_then(Value::as_str),
            ) {
                latest
                    .entry(revision)
                    .or_default()
                    .insert(research, assessment);
            }
        }
        latest.entry(current).or_default();
        let status = text(hypothesis, "status").unwrap_or("unknown");
        for (revision, results) in latest.iter().rev() {
            let verdicts: BTreeSet<&str> = results
                .values()
                .filter_map(|assessment| assessment.get("verdict").and_then(Value::as_str))
                .collect();
            let disputed = verdicts.len() > 1;
            let label = match (*revision == current, disputed) {
                (true, true) => format!("{revision} (current, disputed)"),
                (true, false) => format!("{revision} (current)"),
                (false, true) => format!("{revision} (superseded, disputed)"),
                (false, false) => format!("{revision} (superseded)"),
            };
            let base = |verdict: Value, research: Value, when: Value| {
                json!({
                    "id": hypothesis.id,
                    "name": title_cell(hypothesis),
                    "research": research,
                    "revision": label,
                    "status": status,
                    "verdict": verdict,
                    "when": when,
                })
            };
            if results.is_empty() {
                rows.push(base(json!("not assessed"), Value::Null, Value::Null));
            }
            for (research, assessment) in results {
                let verdict = assessment
                    .get("verdict")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let verdict = match assessment.get("strength").and_then(Value::as_str) {
                    Some(strength) => format!("{verdict} ({strength})"),
                    None => verdict.to_owned(),
                };
                rows.push(base(
                    json!(verdict),
                    json!(research),
                    date_cell(assessment.get("date").and_then(Value::as_str)),
                ));
            }
        }
    }
    if rows.is_empty() {
        return status_rows(
            "No hypotheses yet. Create one with `orbit-research research create --kind H`.",
        );
    }
    Value::Array(rows)
}

// ---- Corpus health --------------------------------------------------------

/// Statuses of each kind in the order a reader expects them; any status the
/// owner schema adds later follows in alphabetical order.
const STATUS_ORDER: [(&str, &[&str]); 4] = [
    ("Q", &["open", "answered", "dropped"]),
    (
        "H",
        &["open", "supported", "refuted", "inconclusive", "dropped"],
    ),
    ("R", &["planned", "running", "done", "abandoned"]),
    ("T", &["active", "superseded", "refuted"]),
];

/// Keys sort as: Corpus, Hypotheses, Questions, Records, Results, Revision,
/// Tags, Theories. `Corpus` is `Valid` because a snapshot only exists for a
/// corpus that passed validation; an invalid one never reaches this view.
fn corpus_health(snapshot: &Snapshot) -> Value {
    let mut view = Map::new();
    view.insert("Corpus".into(), json!("Valid"));
    view.insert(
        "Revision".into(),
        json!(snapshot.revision.get(..12).unwrap_or(&snapshot.revision)),
    );
    view.insert("Records".into(), json!(snapshot.records.len()));
    view.insert("Tags".into(), json!(snapshot.tags.len()));
    for (kind, label) in [
        ("Q", "Questions"),
        ("H", "Hypotheses"),
        ("R", "Results"),
        ("T", "Theories"),
    ] {
        view.insert(label.into(), json!(kind_summary(snapshot, kind)));
    }
    Value::Object(view)
}

/// `4 (2 open, 1 answered, 1 dropped)`, or `0`.
fn kind_summary(snapshot: &Snapshot, kind: &str) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for record in records_of(snapshot, kind) {
        *counts
            .entry(text(record, "status").unwrap_or("unknown"))
            .or_default() += 1;
    }
    let total: usize = counts.values().sum();
    if total == 0 {
        return "0".to_owned();
    }
    let known = STATUS_ORDER
        .iter()
        .find(|(candidate, _)| *candidate == kind)
        .map_or(&[][..], |(_, statuses)| *statuses);
    let mut parts = Vec::new();
    for status in known {
        if let Some(count) = counts.remove(status) {
            parts.push(format!("{count} {status}"));
        }
    }
    parts.extend(
        counts
            .iter()
            .map(|(status, count)| format!("{count} {status}")),
    );
    format!("{total} ({})", parts.join(", "))
}
