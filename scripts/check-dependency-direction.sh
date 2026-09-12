#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
metadata=$(cargo metadata --format-version 1 --no-deps --manifest-path "$repo_root/Cargo.toml")

ORBIT_RESEARCH_CARGO_METADATA="$metadata" python3 - "${1:-}" <<'PY'
import json
import os
import sys

packages = json.loads(os.environ["ORBIT_RESEARCH_CARGO_METADATA"])["packages"]

workspace = {package["name"] for package in packages}
dependencies = {
    package["name"]: {dependency["name"] for dependency in package["dependencies"]}
    for package in packages
}

forbidden = {
    "orbit-research-import": {"orbit-research-owner"},
    "orbit-research-index": {"orbit-research-import"},
    "orbit-research-contract": {
        "orbit-research-owner",
        "orbit-research-import",
        "orbit-research-index",
        "orbit-research-cli",
    },
}

def check(graph):
    problems = []
    for package, forbidden_edges in forbidden.items():
        for dependency in sorted(graph.get(package, set()) & forbidden_edges):
            problems.append(f"forbidden dependency: {package} -> {dependency}")
    for package, direct in graph.items():
        for dependency in sorted(direct):
            if dependency.startswith("orbit-") and dependency not in workspace:
                problems.append(f"forbidden Orbit library dependency: {package} -> {dependency}")
    return problems

if sys.argv[1] == "--self-test":
    cases = [
        ("orbit-research-import", "orbit-research-owner"),
        ("orbit-research-index", "orbit-research-import"),
        ("orbit-research-contract", "orbit-research-owner"),
        ("orbit-research-contract", "orbit-research-import"),
        ("orbit-research-contract", "orbit-research-index"),
        ("orbit-research-contract", "orbit-research-cli"),
    ]
    for package, dependency in cases:
        synthetic = {name: set(direct) for name, direct in dependencies.items()}
        synthetic.setdefault(package, set()).add(dependency)
        if not check(synthetic):
            raise SystemExit(f"self-test failed to reject {package} -> {dependency}")

problems = check(dependencies)
if problems:
    print("\n".join(problems), file=sys.stderr)
    raise SystemExit(1)
PY
