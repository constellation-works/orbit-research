//! `plugin.yaml`'s `list`/`show`/`check` tools must advertise exactly the
//! operation registry's own derived schema, per ARCHITECTURE.md's operation
//! contract: "Adding a tool requires a typed request, handler and registry
//! entry, rather than a handwritten JSON schema". A schema file that drifts
//! from `Operation::definition()` would let the plugin manifest silently
//! diverge from what Core actually accepts.
use orbit_research_core::application::Operation;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn registry_schema(operation: Operation) -> Value {
    serde_json::to_value(operation.definition().input_schema).expect("serialize derived schema")
}

fn committed_schema(file: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("schemas")
        .join(file);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn list_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("list.request.json"),
        registry_schema(Operation::List),
        "schemas/list.request.json has drifted from Operation::List; regenerate it from \
         `Operation::List.definition().input_schema`"
    );
}

#[test]
fn check_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("check.request.json"),
        registry_schema(Operation::Check),
        "schemas/check.request.json has drifted from Operation::Check; regenerate it from \
         `Operation::Check.definition().input_schema`"
    );
}

#[test]
fn show_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("show.request.json"),
        registry_schema(Operation::Show),
        "schemas/show.request.json has drifted from Operation::Show; regenerate it from \
         `Operation::Show.definition().input_schema`"
    );
}

/// `version` reports plugin/protocol versions rather than corpus data, so it
/// has no `Operation` of its own (its handler must not require an openable
/// corpus, see `src/plugin.rs`). Its contract is still "no input", so it is
/// pinned to the same empty-object shape `list`/`check` derive from `Empty`,
/// rather than a hand-invented schema.
#[test]
fn version_schema_matches_the_shared_empty_input_shape() {
    assert_eq!(
        committed_schema("version.request.json"),
        registry_schema(Operation::List),
        "schemas/version.request.json must match the registry's `Empty` input shape"
    );
}
