use super::super::{Operation, api};
use serde_json::{Value, json};
use std::collections::HashSet;

fn schema(operation: Operation) -> jsonschema::JSONSchema {
    let value = serde_json::to_value(operation.definition().input_schema)
        .expect("serialize derived schema");
    jsonschema::JSONSchema::compile(&value).expect("compile derived schema")
}

#[test]
fn every_advertised_operation_has_one_exact_wire_name() {
    let tools = api::tools();
    let tools = tools.as_array().expect("tool list");
    let mut names = HashSet::new();
    assert_eq!(tools.len(), Operation::ALL.len());
    for tool in tools {
        let name = tool["name"].as_str().expect("tool name");
        assert!(names.insert(name), "duplicate tool: {name}");
        let operation: Operation = name.parse().expect("advertised name must route");
        assert_eq!(operation.as_str(), name);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    assert!("research.revise-question".parse::<Operation>().is_err());
    assert!("research.UNKNOWN".parse::<Operation>().is_err());
}

#[test]
fn create_schema_tracks_defaults_kinds_and_required_fields() {
    let schema = schema(Operation::Create);
    let minimum = json!({"request_key": "capture-1", "kind": "Q", "title": "Question"});
    assert!(schema.is_valid(&minimum));
    let mut invalid = minimum.clone();
    invalid["kind"] = json!("assessment");
    assert!(!schema.is_valid(&invalid));
    for required in ["request_key", "kind", "title"] {
        let mut invalid = minimum.clone();
        invalid.as_object_mut().expect("object").remove(required);
        assert!(!schema.is_valid(&invalid), "missing {required}");
    }
    let mut invalid = minimum;
    invalid["title"] = json!("");
    assert!(!schema.is_valid(&invalid));
    invalid["title"] = json!("Question");
    invalid["root"] = json!("/another/owner");
    assert!(!schema.is_valid(&invalid));
}

#[test]
fn linked_plan_schema_is_derived_from_the_actual_work_plan() {
    let schema = schema(Operation::LinkWork);
    let mut request = json!({
        "request_key": "link-1", "title": "Measure", "crew": "luna",
        "plan": {
            "research_id": "R001", "corpus_revision": "revision",
            "research_blob": "blob", "mode": "contribution",
            "context_files": ["dir:research/R001-measure/code/unit"],
            "instructions": "Measure the baseline."
        }
    });
    assert!(schema.is_valid(&request));
    request["plan"]["mode"] = json!("invented");
    assert!(!schema.is_valid(&request));
    request["plan"]["mode"] = json!("contribution");
    request["plan"]["authority"] = Value::Bool(true);
    assert!(!schema.is_valid(&request));
}
