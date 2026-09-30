#!/usr/bin/env python3
"""Build a locked public Store API benchmark outside the source checkout."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def git_environment():
    """Keep ambient Git overrides from redirecting private fixture writes."""
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(
        GIT_CONFIG_NOSYSTEM="1",
        GIT_CONFIG_GLOBAL=os.devnull,
        GIT_AUTHOR_NAME="Benchmark",
        GIT_AUTHOR_EMAIL="benchmark@example.invalid",
        GIT_COMMITTER_NAME="Benchmark",
        GIT_COMMITTER_EMAIL="benchmark@example.invalid",
        GIT_AUTHOR_DATE="2026-01-01T00:00:00Z",
        GIT_COMMITTER_DATE="2026-01-01T00:00:00Z",
    )
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repository", type=Path)
    parser.add_argument("--revision", help="label for a clean source export without .git")
    parser.add_argument("--records", type=int, default=999)
    parser.add_argument("--samples", type=int, default=21)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.records <= 999:
        parser.error("--records must be between 1 and 999")
    if args.samples < 1 or args.samples % 2 != 1:
        parser.error("--samples must be a positive odd count")
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    repository = args.repository.resolve()
    if not (repository / "crates/orbit-research-store/Cargo.toml").is_file():
        parser.error("repository must contain orbit-research-store")
    harness = Path(__file__).resolve().parent
    project = Path(tempfile.mkdtemp(prefix="research-committed-benchmark-"))
    source = project / "source"
    source.mkdir()
    for name in ["Cargo.toml", "Cargo.lock"]:
        shutil.copy2(repository / name, source / name)
    shutil.copytree(repository / "crates", source / "crates")
    fingerprint = hashlib.sha256()
    for path in sorted(source.rglob("*")):
        if path.is_file():
            fingerprint.update(path.relative_to(source).as_posix().encode())
            fingerprint.update(path.read_bytes())
    shutil.copytree(harness / "src", project / "src")
    shutil.copy2(harness / "Cargo.lock", project / "Cargo.lock")
    (project / "Cargo.toml").write_text(
        '[package]\nname = "research-committed-benchmark"\n'
        'version = "0.0.0"\nedition = "2024"\n[dependencies]\n'
        'orbit-research-store = { path = "source/crates/orbit-research-store" }\n'
        'serde_json = "=1.0.151"\n'
    )
    fixture = project / "fixture"
    for directory in ["_scripts", "questions", "hypotheses", "theories", "research"]:
        (fixture / directory).mkdir(parents=True)
    shutil.copy2(
        source / "crates/orbit-research-store/src/tests/fixtures/schema.json",
        fixture / "_scripts/schema.json",
    )
    for number in range(1, args.records + 1):
        (fixture / "questions" / f"Q{number:03}-question-{number:03}.md").write_text(
            f"---\nid: Q{number:03}\ntitle: Question {number:03}\nstatus: open\n"
            "tags: [bench]\nderived_from: []\ncreated: 2026-01-01\n"
            "updated: 2026-01-01\nanswered_by: []\n---\n"
            f"Benchmark body {number}.\n"
        )
    git_env = git_environment()
    for command in [
        ["init", "-q", "--initial-branch=main"],
        ["config", "user.name", "Benchmark"],
        ["config", "user.email", "benchmark@example.invalid"],
        ["add", "."],
        ["commit", "-q", "-m", "fixture"],
    ]:
        subprocess.run(["git", "-C", str(fixture)] + command, env=git_env, check=True, timeout=60)
    env = dict(git_env, CARGO_TARGET_DIR=str(project / "target"))
    revision = args.revision
    if revision is None:
        resolved = subprocess.run(
            ["git", "-C", str(repository), "rev-parse", "HEAD"],
            env=git_env, capture_output=True, text=True, timeout=10,
        )
        revision = resolved.stdout.strip() if resolved.returncode == 0 else "source export"
    print(json.dumps({
        "project": str(project), "source_repository": str(repository),
        "source_revision": revision, "source_fingerprint_sha256": fingerprint.hexdigest(),
        "lock_sha256": hashlib.sha256((project / "Cargo.lock").read_bytes()).hexdigest(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True, timeout=10).strip(),
        "profile": "release", "statistic": "median of odd sample count", "unit": "milliseconds",
    }), flush=True)
    build = ["cargo", "build", "--release", "--locked"]
    if args.offline:
        build.append("--offline")
    subprocess.run(build, cwd=project, env=env, check=True, timeout=300)
    for repeat in range(args.repeats):
        output = subprocess.check_output([
            str(project / "target/release/research-committed-benchmark"),
            str(fixture), str(args.records), str(args.samples),
        ], env=env, text=True, timeout=120)
        for line in output.splitlines():
            result = json.loads(line)
            result["repeat"] = repeat + 1
            print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
