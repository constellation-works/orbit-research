//! Native scientific guards for schema v2 appends.
//!
//! These are pure functions over already-parsed records: registration chronology, frozen
//! protocol arithmetic, typed native edges and the confirmatory-primary closure. They are
//! the Rust reading of `science.py` and the owner enforces them at the write boundary.
//! Design §2.2 places scientific invariants in `orbit-research-contract`; the contract
//! crate does not carry the v2 guards yet, so they live here until that slice lands and
//! `contract::validate` can run them for every caller.

use std::collections::{HashMap, HashSet};

use orbit_research_contract::record_references;
use serde_json::Value;

use crate::time::instant;

/// `(id, revision_id, source_revision)` — the exact identity every native edge keys on.
pub type RecordKey = (String, String, Option<String>);

const MODEL_SCOPES: &[&str] = &[
    "derivation",
    "simulation-under-assumptions",
    "synthetic-calibration",
];

/// The exact key of a record snapshot.
pub fn record_key(record: &Value) -> RecordKey {
    (
        text(record, "id").unwrap_or_default().to_owned(),
        text(record, "revision_id").unwrap_or_default().to_owned(),
        record
            .get("provenance")
            .and_then(|value| value.get("git_revision"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    )
}

/// The exact key a reference points at.
pub fn reference_key(reference: &Value) -> RecordKey {
    (
        text(reference, "id").unwrap_or_default().to_owned(),
        text(reference, "revision_id")
            .unwrap_or_default()
            .to_owned(),
        reference
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned),
    )
}

/// Explicit applicability, finite budgets and attainable decision arithmetic.
pub fn protocol_errors(semantic: &Value) -> Vec<String> {
    check_protocol(semantic)
        .err()
        .map(|error| vec![format!("protocol: {error}")])
        .unwrap_or_default()
}

fn check_protocol(semantic: &Value) -> Result<(), String> {
    for field in [
        "question",
        "assumptions",
        "analysis",
        "exclusions",
        "stopping_rule",
    ] {
        demand(
            text(semantic, field).is_some_and(|value| !value.trim().is_empty()),
            &format!("protocol requires {field}"),
        )?;
    }
    demand(
        array(semantic, "claims").is_some_and(|claims| !claims.is_empty()),
        "protocol requires exact claim references",
    )?;
    let code = field(semantic, "code")?;
    demand(
        text(code, "repository").is_some_and(|value| !value.is_empty())
            && text(code, "git_revision").is_some_and(crate::git::full_revision),
        "protocol requires exact code revision",
    )?;
    demand(
        array(semantic, "inputs").is_some(),
        "protocol inputs must be explicit",
    )?;
    let design = field(semantic, "design")?;
    let kind = text(design, "kind").unwrap_or_default();
    demand(
        ["empirical", "synthetic", "deterministic"].contains(&kind),
        "unsupported design kind",
    )?;
    let budget = field(design, "resource_budget")?;
    let planned = finite(budget, "planned");
    let limit = finite(budget, "limit");
    demand(
        planned.is_some_and(|planned| limit.is_some_and(|limit| (0.0..=limit).contains(&planned)))
            && text(budget, "unit").is_some_and(|unit| !unit.is_empty()),
        "incoherent resource budget",
    )?;
    if kind == "deterministic" {
        let convergence = field(design, "convergence")?;
        demand(
            finite(convergence, "tolerance").is_some_and(|tolerance| tolerance > 0.0)
                && integer(convergence, "max_steps").is_some_and(|steps| steps > 0)
                && text(convergence, "criterion").is_some_and(|value| !value.is_empty()),
            "deterministic work requires convergence criterion, tolerance and step budget",
        )?;
        demand(
            truthy(design.get("decision")),
            "deterministic decision rule required",
        )?;
        return Ok(());
    }
    let samples = field(design, "samples")?;
    let groups = array(samples, "groups").unwrap_or_default();
    demand(
        !groups.is_empty()
            && groups.iter().all(|group| {
                integer(group, "count").is_some_and(|count| count > 0)
                    && text(group, "name").is_some_and(|name| !name.is_empty())
            }),
        "positive sample counts required",
    )?;
    let total: i64 = groups
        .iter()
        .filter_map(|group| integer(group, "count"))
        .sum();
    demand(
        integer(samples, "total") == Some(total),
        "sample total does not match enumerated groups",
    )?;
    let names: HashSet<_> = groups
        .iter()
        .filter_map(|group| text(group, "name"))
        .collect();
    demand(names.len() == groups.len(), "duplicate sample group")?;
    demand(
        truthy(design.get("controls")) && truthy(design.get("baseline")),
        "empirical/synthetic design requires controls and baseline",
    )?;
    let rule = field(design, "decision")?;
    let threshold = finite(rule, "threshold");
    let minimum = finite(rule, "attainable_min");
    let maximum = finite(rule, "attainable_max");
    demand(
        threshold.is_some() && minimum.is_some() && maximum.is_some(),
        "decision range must be finite",
    )?;
    let (threshold, minimum, maximum) = (
        threshold.unwrap_or_default(),
        minimum.unwrap_or_default(),
        maximum.unwrap_or_default(),
    );
    demand(
        minimum <= threshold && threshold <= maximum,
        "decision threshold is unattainable",
    )?;
    let operator = text(rule, "operator").unwrap_or_default();
    demand(
        [">=", "<=", ">", "<"].contains(&operator)
            && text(rule, "metric").is_some_and(|metric| !metric.is_empty()),
        "explicit decision metric/operator required",
    )?;
    demand(
        !(operator == ">" && threshold == maximum || operator == "<" && threshold == minimum),
        "strict threshold is unattainable",
    )?;
    if let Some(bound) = rule.get("binomial_lower_bound") {
        let samples = integer(bound, "n");
        let confidence = finite(bound, "confidence");
        demand(
            samples.is_some_and(|n| 0 < n && n <= total)
                && confidence.is_some_and(|value| 0.0 < value && value < 1.0),
            "invalid binomial sample/confidence",
        )?;
        // Exact one-sided all-success Clopper-Pearson bound: the best possible result.
        let best =
            (1.0 - confidence.unwrap_or_default()).powf(1.0 / samples.unwrap_or(1).max(1) as f64);
        demand(
            [">=", ">"].contains(&operator)
                && if operator == ">" {
                    best > threshold
                } else {
                    best >= threshold
                },
            "binomial lower-bound threshold unattainable even with all successes",
        )?;
    }
    let holdout = field(semantic, "holdout")?;
    demand(
        text(holdout, "digest").is_some_and(is_digest),
        "immutable holdout/seed-plan digest required",
    )?;
    demand(
        truthy(holdout.get("policy")),
        "holdout access policy required",
    )?;
    demand(
        moment(holdout, "information_cutoff")? <= moment(holdout, "evaluation_not_before")?,
        "holdout cutoff after evaluation boundary",
    )
}

/// Chronology, measured freeze and evidence-summary limits for one native record.
pub fn native_errors(record: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(error) = check_native(record, &mut errors) {
        errors.push(format!("native: {error}"));
    }
    errors
}

fn check_native(record: &Value, errors: &mut Vec<String>) -> Result<(), String> {
    let authorship = field(record, "authorship")?;
    let payload = field(record, "payload")?;
    let registered_at = text(authorship, "registered_at").unwrap_or_default();
    let registered = point(registered_at)?;
    demand(
        record
            .get("provenance")
            .and_then(|value| value.get("historical"))
            .and_then(Value::as_bool)
            == Some(false)
            && record.get("legacy").is_some_and(Value::is_null),
        "native authoring cannot relabel imported history",
    )?;
    demand(
        array(record, "orbit_links")
            .is_some_and(|links| links.iter().all(|link| truthy(link.get("run")))),
        "native provenance requires host/workspace/task/run",
    )?;
    match text(record, "kind").unwrap_or_default() {
        "protocol" => {
            let semantic = field(payload, "semantic")?;
            errors.extend(protocol_errors(semantic));
            demand(
                text(payload, "freeze") == Some("registered")
                    && text(payload, "frozen_at") == Some(registered_at)
                    && payload.get("freeze_evidence").is_some_and(Value::is_null),
                "native freeze is measured registration, not an asserted prospective timestamp",
            )?;
            let holdout = field(semantic, "holdout")?;
            demand(
                moment(holdout, "information_cutoff")? <= registered
                    && registered <= moment(holdout, "evaluation_not_before")?,
                "freeze must precede the evaluation boundary and follow information cutoff",
            )?;
        }
        "experiment" => {
            let started = text(payload, "started_at");
            let finished = text(payload, "finished_at");
            if let Some(started) = started {
                demand(
                    point(started)? <= registered,
                    "run start after registration",
                )?;
            }
            if let Some(finished) = finished {
                demand(
                    started.is_none_or(|started| {
                        point(started)
                            .is_ok_and(|start| point(finished).is_ok_and(|finish| start <= finish))
                    }),
                    "finish before start",
                )?;
                demand(
                    point(finished)? == registered,
                    "finish must be measured at registration",
                )?;
            }
            if text(payload, "execution_status") == Some("running") {
                demand(
                    started == Some(registered_at)
                        && finished.is_none()
                        && payload.get("start").is_some_and(Value::is_null),
                    "invalid run-start receipt",
                )?;
            }
        }
        "assessment" => {
            let summary = text(payload, "evidence_summary").unwrap_or_default();
            let verdict = text(payload, "verdict").unwrap_or_default();
            demand(
                !["mixed", "inconclusive", "unmeasured"].contains(&summary)
                    || !["supported", "refuted"].contains(&verdict),
                "mixed or unmeasured evidence requires a limited verdict",
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Typed native edges and the model/nature boundary, which apply to exploratory work too.
pub fn native_reference_errors(record: &Value, known: &HashMap<RecordKey, Value>) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(payload) = record.get("payload") else {
        return errors;
    };
    let get = |reference: &Value| known.get(&reference_key(reference));
    let mut edges: Vec<(&Value, &str)> = Vec::new();
    match text(record, "kind").unwrap_or_default() {
        "protocol" => {
            let semantic = payload.get("semantic").unwrap_or(&Value::Null);
            edges.extend(
                array(semantic, "claims")
                    .unwrap_or_default()
                    .iter()
                    .map(|reference| (reference, "claim")),
            );
            edges.extend(
                array(semantic, "inputs")
                    .unwrap_or_default()
                    .iter()
                    .map(|reference| (reference, "artifact")),
            );
        }
        "experiment" => {
            if let Some(protocol) = payload.get("protocol").filter(|value| !value.is_null()) {
                edges.push((protocol, "protocol"));
            }
            for reference in array(payload, "inputs").unwrap_or_default().iter().chain(
                array(payload, "result_artifacts")
                    .unwrap_or_default()
                    .iter(),
            ) {
                edges.push((reference, "artifact"));
            }
            for reference in array(payload, "result_artifacts").unwrap_or_default() {
                if get(reference).is_some_and(|target| {
                    text(target, "kind") == Some("artifact")
                        && target.get("payload").and_then(|p| text(p, "role")) != Some("result")
                }) {
                    errors.push("run output must reference a result artifact".to_owned());
                }
            }
        }
        "assessment" => {
            let claim_reference = payload.get("claim").unwrap_or(&Value::Null);
            edges.push((claim_reference, "claim"));
            let claim = get(claim_reference);
            let about_nature = claim.is_some_and(|claim| {
                text(claim, "kind") == Some("claim")
                    && claim.get("payload").and_then(|p| text(p, "domain")) == Some("nature")
            });
            if about_nature
                && ["supported", "refuted"].contains(&text(payload, "verdict").unwrap_or_default())
            {
                let mut seen: HashSet<(String, String)> = HashSet::new();
                let mut queue = vec![record.clone()];
                while let Some(current) = queue.pop() {
                    let key = (
                        text(&current, "id").unwrap_or_default().to_owned(),
                        text(&current, "revision_id").unwrap_or_default().to_owned(),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    if MODEL_SCOPES.contains(&text(&current, "scope").unwrap_or_default()) {
                        errors.push(
                            "model or synthetic scope cannot establish a verdict about nature"
                                .to_owned(),
                        );
                    }
                    queue.extend(
                        record_references(&current)
                            .into_iter()
                            .filter_map(|reference| get(reference).cloned()),
                    );
                }
            }
        }
        _ => {}
    }
    for (reference, kind) in edges {
        if let Some(target) = get(reference)
            && text(target, "kind") != Some(kind)
        {
            errors.push(format!(
                "native reference requires {kind}, found {}",
                text(target, "kind").unwrap_or_default()
            ));
        }
    }
    errors
}

/// Every dependency, including indirect controls, must be usable; no latest-wins.
pub fn confirmation_errors(
    assessment: &Value,
    known: &HashMap<RecordKey, Value>,
    verified_artifacts: &HashSet<RecordKey>,
) -> Vec<String> {
    let mut state = Confirmation {
        known,
        verified_artifacts,
        errors: Vec::new(),
        visited: HashSet::new(),
    };
    let payload = assessment.get("payload").unwrap_or(&Value::Null);
    let claim = state
        .target(payload.get("claim").unwrap_or(&Value::Null))
        .cloned();
    if claim
        .as_ref()
        .is_some_and(|claim| text(claim, "kind") != Some("claim"))
    {
        state
            .errors
            .push("assessment must target a claim".to_owned());
    }
    if assessment.get("schema_version").and_then(Value::as_u64) == Some(2) {
        let summary = text(payload, "evidence_summary").unwrap_or_default();
        if ["mixed", "inconclusive", "unmeasured"].contains(&summary) {
            state
                .errors
                .push("mixed or unmeasured evidence cannot yield primary confirmation".to_owned());
        }
        let verdict = text(payload, "verdict").unwrap_or_default();
        if !matches!(
            (summary, verdict),
            ("supports", "supported") | ("refutes", "refuted")
        ) {
            state
                .errors
                .push("confirmation verdict must agree with evidence summary".to_owned());
        }
    }
    let evidence: Vec<Value> = array(payload, "evidence").unwrap_or_default().to_vec();
    for reference in &evidence {
        state.visit(reference, true, claim.as_ref(), assessment);
    }
    // An artifact-only assertion has no checked control or chronology path.
    if !evidence.iter().any(|reference| {
        known
            .get(&reference_key(reference))
            .is_some_and(|target| text(target, "kind") == Some("experiment"))
    }) {
        state
            .errors
            .push("primary confirmation requires an assessed experiment".to_owned());
    }
    state.errors
}

struct Confirmation<'a> {
    known: &'a HashMap<RecordKey, Value>,
    verified_artifacts: &'a HashSet<RecordKey>,
    errors: Vec<String>,
    visited: HashSet<RecordKey>,
}

impl<'a> Confirmation<'a> {
    /// An exact, resolved, committed target; working-tree or unpinned evidence is refused.
    fn target(&mut self, reference: &Value) -> Option<&'a Value> {
        let known = self.known;
        let key = reference_key(reference);
        let record = known.get(&key);
        let usable = text(reference, "status") == Some("resolved")
            && record.is_some_and(|record| {
                self.verified_artifacts.contains(&key)
                    || (key.2.is_some()
                        && record
                            .get("provenance")
                            .and_then(|value| value.get("working_tree"))
                            .and_then(Value::as_bool)
                            == Some(false))
            });
        if !usable {
            self.errors
                .push("confirmation requires an exact resolved evidence closure".to_owned());
            return None;
        }
        record
    }

    fn visit(
        &mut self,
        reference: &Value,
        evidence: bool,
        claim: Option<&Value>,
        assessment: &Value,
    ) {
        let Some(record) = self.target(reference) else {
            return;
        };
        let key = reference_key(reference);
        if !self.visited.insert(key) {
            return;
        }
        let payload = record.get("payload").unwrap_or(&Value::Null);
        let kind = text(record, "kind").unwrap_or_default();
        let historical = record
            .get("provenance")
            .and_then(|value| value.get("historical"))
            .and_then(Value::as_bool)
            == Some(true);
        if evidence && historical && ["experiment", "assessment"].contains(&kind) {
            self.errors.push(
                "historical evidence cannot establish native primary confirmation".to_owned(),
            );
        }
        if evidence
            && claim.is_some_and(|claim| {
                claim.get("payload").and_then(|p| text(p, "domain")) == Some("nature")
            })
            && MODEL_SCOPES.contains(&text(record, "scope").unwrap_or_default())
        {
            self.errors
                .push("model or synthetic evidence cannot confirm a claim about nature".to_owned());
        }
        if kind == "artifact" {
            if text(payload, "availability") != Some("available") {
                self.errors
                    .push("unavailable artifact cannot support confirmation".to_owned());
            }
            if array(record, "missingness")
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .any(|missing| missing.contains("lineage"))
            {
                self.errors.push(
                    "unresolved or unrecorded dataset lineage blocks confirmation".to_owned(),
                );
            }
        }
        if kind == "experiment" && evidence {
            self.check_run(record, payload, assessment);
        }
        if kind == "assessment"
            && evidence
            && !["supported", "refuted"].contains(&text(payload, "verdict").unwrap_or_default())
        {
            self.errors
                .push("mixed/limited assessment cannot support primary confirmation".to_owned());
        }
        let supersedes: Vec<Value> = record
            .get("authorship")
            .and_then(|authorship| array(authorship, "supersedes"))
            .unwrap_or_default()
            .to_vec();
        let start = payload.get("start").cloned().unwrap_or(Value::Null);
        for child in record_references(record) {
            // Superseded records are history, not current evidence. Start is checked above.
            if supersedes.contains(child) || *child == start {
                self.target(child);
                continue;
            }
            self.visit(child, evidence, claim, assessment);
        }
    }

    /// A run-start receipt is operational lineage; a result needs the full frozen chronology.
    fn check_run(&mut self, record: &Value, payload: &Value, assessment: &Value) {
        if text(payload, "execution_status") != Some("completed")
            || !["passed", "not-applicable"]
                .contains(&text(payload, "controls").unwrap_or_default())
        {
            self.errors.push(
                "failed/pending execution or controls cannot support confirmation".to_owned(),
            );
        }
        if payload.get("protocol").is_none_or(Value::is_null)
            || array(payload, "result_artifacts").is_none_or(<[Value]>::is_empty)
        {
            self.errors
                .push("confirmation requires protocol and result artifacts".to_owned());
        }
        if record.get("schema_version").and_then(Value::as_u64) != Some(2)
            || payload.get("start").is_none_or(Value::is_null)
            || array(payload, "deviations").is_some_and(|values| !values.is_empty())
        {
            self.errors.push(
                "confirmation requires native run-start chronology without deviations".to_owned(),
            );
            return;
        }
        let start = payload.get("start").cloned().unwrap_or(Value::Null);
        let protocol_reference = payload.get("protocol").cloned().unwrap_or(Value::Null);
        let start = self.target(&start).cloned();
        let protocol = if protocol_reference.is_null() {
            None
        } else {
            self.target(&protocol_reference).cloned()
        };
        let Some(start) = start.filter(|start| {
            text(start, "kind") == Some("experiment")
                && start
                    .get("payload")
                    .and_then(|p| text(p, "execution_status"))
                    == Some("running")
        }) else {
            self.errors
                .push("confirmation requires exact running receipt".to_owned());
            return;
        };
        let Some(protocol) = protocol.filter(|protocol| {
            protocol.get("schema_version").and_then(Value::as_u64) == Some(2)
                && text(protocol, "kind") == Some("protocol")
        }) else {
            self.errors
                .push("confirmation requires native registered protocol".to_owned());
            return;
        };
        let start_payload = start.get("payload").unwrap_or(&Value::Null);
        let protocol_payload = protocol.get("payload").unwrap_or(&Value::Null);
        let semantic = protocol_payload.get("semantic").unwrap_or(&Value::Null);
        let design = semantic.get("design").unwrap_or(&Value::Null);
        if text(design, "kind") != Some("deterministic") {
            let control_results = payload.get("control_results").unwrap_or(&Value::Null);
            if text(payload, "controls") != Some("passed")
                || array(design, "controls")
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|name| text(control_results, name) != Some("passed"))
            {
                self.errors
                    .push("every frozen control must explicitly pass".to_owned());
            }
        } else if text(payload, "controls") == Some("not-applicable")
            && payload
                .get("control_results")
                .and_then(Value::as_object)
                .is_some_and(|results| !results.is_empty())
        {
            self.errors
                .push("inapplicable controls cannot carry measured control results".to_owned());
        }
        if start_payload.get("protocol") != payload.get("protocol")
            || start_payload.get("code") != semantic.get("code")
            || payload.get("code") != start_payload.get("code")
            || payload.get("inputs") != start_payload.get("inputs")
            || payload.get("inputs") != semantic.get("inputs")
            || payload.get("holdout_digest")
                != semantic
                    .get("holdout")
                    .and_then(|holdout| holdout.get("digest"))
            || start_payload.get("holdout_digest") != payload.get("holdout_digest")
        {
            self.errors
                .push("run code/data/holdout pins differ from frozen protocol".to_owned());
        }
        let chronology = (|| -> Result<bool, String> {
            let started_at = text(start_payload, "started_at").unwrap_or_default();
            Ok(
                point(text(protocol_payload, "frozen_at").unwrap_or_default())?
                    <= point(started_at)?
                    && moment(
                        semantic.get("holdout").unwrap_or(&Value::Null),
                        "evaluation_not_before",
                    )? <= point(started_at)?
                    && text(payload, "started_at") == Some(started_at),
            )
        })();
        if chronology != Ok(true) {
            self.errors
                .push("freeze/start/holdout chronology mismatch".to_owned());
        }
        let claim_reference = assessment
            .get("payload")
            .and_then(|payload| payload.get("claim"))
            .unwrap_or(&Value::Null);
        if !array(semantic, "claims")
            .unwrap_or_default()
            .iter()
            .any(|frozen| {
                text(frozen, "id") == text(claim_reference, "id")
                    && text(frozen, "revision_id") == text(claim_reference, "revision_id")
            })
        {
            self.errors
                .push("protocol did not freeze this claim revision".to_owned());
        }
    }
}

fn demand(ok: bool, message: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(message.to_owned()) }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value
        .get(key)
        .filter(|found| !found.is_null())
        .ok_or_else(|| format!("missing {key}"))
}

fn moment(value: &Value, key: &str) -> Result<i128, String> {
    point(text(value, key).unwrap_or_default())
}

fn point(value: &str) -> Result<i128, String> {
    instant(value).map_err(|error| error.to_string())
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
    }
}

fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn finite(value: &Value, key: &str) -> Option<f64> {
    match value.get(key) {
        Some(Value::Number(number)) => number.as_f64().filter(|value| value.is_finite()),
        _ => None,
    }
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    match value.get(key) {
        Some(Value::Number(number)) => number.as_i64(),
        _ => None,
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn array<'a>(value: &'a Value, key: &str) -> Option<&'a [Value]> {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice)
}
