#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: scripts/check-dependency-direction.sh [--self-test] [-h|--help]

Checks that the workspace crates depend on each other only in the accepted
direction (ARCHITECTURE.md). With --self-test it first proves the checker
itself rejects every forbidden edge. Needs `cargo` and `python3` on PATH.
Exit status: 0 when the graph is accepted, 1 when it is not, 2 for a usage
error.
EOF
}

self_test=0
case "$#:${1:-}" in
    0:) ;;
    1:-h | 1:--help) usage; exit 0 ;;
    1:--self-test) self_test=1 ;;
    *)
        printf 'check-dependency-direction: unknown argument: %s\n' "$*" >&2
        usage >&2
        exit 2
        ;;
esac

for program in cargo python3; do
    command -v "$program" >/dev/null 2>&1 || {
        printf 'check-dependency-direction: `%s` was not found on PATH; install it (Rust toolchain: https://rustup.rs) and retry\n' "$program" >&2
        exit 1
    }
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
metadata=$(cargo metadata --format-version 1 --no-deps --manifest-path "$repo_root/Cargo.toml")
ORBIT_RESEARCH_CARGO_METADATA="$metadata" python3 - "$self_test" <<'PY'
import json
import os
import sys

allowed = {
    "orbit-research-common": set(),
    "orbit-research-store": {"orbit-research-common"},
    "orbit-research-core": {"orbit-research-common", "orbit-research-store"},
    "orbit-research-cli": {"orbit-research-core"},
}
packages = json.loads(os.environ["ORBIT_RESEARCH_CARGO_METADATA"])["packages"]
graph = {p["name"]: {d["name"] for d in p["dependencies"]} for p in packages}
def check(graph):
    errors = []
    for name, dependencies in graph.items():
        if name not in allowed:
            errors.append(f"unexpected workspace crate: {name}")
            continue
        for dependency in dependencies:
            if dependency.startswith("orbit-") and dependency not in allowed[name]:
                errors.append(f"forbidden dependency: {name} -> {dependency}")
    return errors

if sys.argv[1] == "1":
    assert not check(allowed), "accepted graph rejected"
    for name, permitted in allowed.items():
        for dependency in set(allowed) - permitted:
            assert check({name: {dependency}}), f"missed edge {name} -> {dependency}"
        assert check({name: {"orbit-core"}}), "embedded Orbit library accepted"
    assert check({"orbit-research-index": set()}), "retired crate accepted"
errors = check(graph)
if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
PY
