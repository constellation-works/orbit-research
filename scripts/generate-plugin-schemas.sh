#!/bin/sh
set -eu

# Regenerate request schemas from the same Rust types used by the drift checks.

usage() {
    cat <<'EOF'
usage: scripts/generate-plugin-schemas.sh [-h|--help]

Rewrites .orbit-plugin/schemas/*.request.json from the Rust request types the
drift checks (crates/orbit-research-cli/tests/plugin_schemas.rs and the plugin
tests in src/tests/plugin.rs) compare them with. Takes no other arguments.
Review the resulting diff and commit it with the type change.
EOF
}

case "${1-}" in
    -h|--help) [ "$#" -eq 1 ] || { usage >&2; exit 2; }; usage; exit 0 ;;
    '') ;;
    *) printf 'generate-plugin-schemas: unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac
[ "$#" -eq 0 ] || { usage >&2; exit 2; }

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$repo_root"
ORBIT_RESEARCH_WRITE_SCHEMAS=1 cargo test --locked -p orbit-research-cli --test plugin_schemas schema_matches
ORBIT_RESEARCH_WRITE_SCHEMAS=1 cargo test --locked -p orbit-research-cli --bin orbit-research input_schema_matches
