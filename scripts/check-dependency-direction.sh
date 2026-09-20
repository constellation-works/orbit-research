#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
metadata=$(cargo metadata --format-version 1 --no-deps --manifest-path "$repo_root/Cargo.toml")
ORBIT_RESEARCH_CARGO_METADATA="$metadata" python3 - "${1:-}" <<'PY'
import json
import os
import sys

allowed = {
    "orbit-research-common": set(),
    "orbit-research-store": {"orbit-research-common"},
    "orbit-research-core": {"orbit-research-common", "orbit-research-store"},
    "orbit-research-web": {"orbit-research-core"},
    "orbit-research-cli": {"orbit-research-core", "orbit-research-web"},
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

if sys.argv[1] == "--self-test":
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
