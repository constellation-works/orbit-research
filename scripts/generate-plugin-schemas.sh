#!/bin/sh
set -eu

# Regenerate request schemas from the same Rust types used by the drift checks.
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"
ORBIT_RESEARCH_WRITE_SCHEMAS=1 cargo test --locked -p orbit-research-cli --test plugin_schemas schema_matches
ORBIT_RESEARCH_WRITE_SCHEMAS=1 cargo test --locked -p orbit-research-cli --bin orbit-research input_schema_matches
