#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: scripts/check-agent-context.sh [--self-test] [--root DIR] [-h|--help]

Checks repository instruction files (AGENTS.md, CLAUDE.md, GEMINI.md) against
STD-11@3 §§R11, R16, R17, R20, and R25:
  - §R11: bullets, steps, and paragraphs <= 500 characters outside fences,
          tables, and constellation-standards managed blocks;
  - §R17: relative Markdown links and backticked repository paths resolve;
  - §R20: AGENTS.md is a regular file per directory; CLAUDE.md/GEMINI.md are
          symlinks to it or files starting with '@AGENTS.md';
  - §§R16/R25: combined directory byte budget <= 8000 (root) or 4000 (subdir).

With --self-test, runs positive and negative fixtures for every rule before
checking the repository. Needs `python3` (and `git` inside a git checkout).
Exit status: 0 when all checks pass, 1 when any check fails, 2 on usage error.
EOF
}

self_test=0
root_arg=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        -h | --help)
            usage
            exit 0
            ;;
        --self-test)
            self_test=1
            shift
            ;;
        --root)
            if [ "$#" -lt 2 ]; then
                printf 'check-agent-context: --root requires a directory argument\n' >&2
                usage >&2
                exit 2
            fi
            root_arg="$2"
            shift 2
            ;;
        --root=*)
            root_arg="${1#--root=}"
            shift
            ;;
        *)
            printf 'check-agent-context: unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

command -v python3 >/dev/null 2>&1 || {
    printf 'check-agent-context: `python3` was not found on PATH\n' >&2
    exit 1
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
target_root="${root_arg:-$repo_root}"

python3 - "$self_test" "$target_root" <<'PY'
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

INSTRUCTION_NAMES = ("AGENTS.md", "CLAUDE.md", "GEMINI.md")
MANAGED_BEGIN = "<!-- constellation-standards:begin -->"
MANAGED_END = "<!-- constellation-standards:end -->"

FENCE_OPEN_RE = re.compile(r"^(```+[^`]*|~~~.*)$")
HEADING_OR_TABLE_RE = re.compile(r"^[ \t]*(?:#|\|)")
LIST_ITEM_RE = re.compile(r"^[ \t]*(?:[-+*]|[0-9]+[.)])[ \t]+")
MD_LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]*)\)")
BACKTICK_RE = re.compile(r"`([^`]+)`")
REPO_PATH_RE = re.compile(r"^([.a-zA-Z0-9_-]+/)+[.a-zA-Z0-9_-]+/?$")


def load_excludes(root: Path) -> list[str]:
    excludes_file = root / "docs" / "standards" / ".standards-skill-excludes"
    if not excludes_file.is_file():
        return []
    excludes: list[str] = []
    for raw in excludes_file.read_text(encoding="utf-8").splitlines():
        item = raw.strip()
        if not item or item.startswith("#"):
            continue
        item = item.removeprefix("./").rstrip("/")
        if item:
            excludes.append(item)
    return excludes


def is_excluded(rel_posix: str, excludes: list[str]) -> bool:
    for ex in excludes:
        if rel_posix == ex or rel_posix.startswith(ex + "/"):
            return True
    return False


def discover_instruction_files(root: Path) -> list[str]:
    excludes = load_excludes(root)
    candidates: list[str] = []

    use_git = False
    try:
        top = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        if top and Path(top).resolve() == root.resolve():
            use_git = True
    except (OSError, subprocess.CalledProcessError):
        use_git = False

    if use_git:
        out = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            check=True,
            capture_output=True,
        ).stdout
        for raw_rel in out.split(b"\0"):
            if not raw_rel:
                continue
            rel = raw_rel.decode("utf-8", errors="surrogateescape")
            if Path(rel).name not in INSTRUCTION_NAMES:
                continue
            full = root / rel
            if os.path.lexists(full) and not is_excluded(rel, excludes):
                candidates.append(rel)
    else:
        for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
            dirnames[:] = sorted(
                d for d in dirnames if d != ".git" and not (Path(dirpath) / d).is_symlink()
            )
            rel_dir = Path(dirpath).relative_to(root)
            for name in sorted(filenames):
                if name not in INSTRUCTION_NAMES:
                    continue
                rel = (rel_dir / name).as_posix() if rel_dir != Path(".") else name
                if not is_excluded(rel, excludes):
                    candidates.append(rel)

    return sorted(set(candidates))


def read_first_line(path: Path) -> str:
    with path.open("rb") as fh:
        raw = fh.readline()
    return raw.decode("utf-8", errors="replace").rstrip("\r\n")


def check_paragraph_lengths(rel_path: str, text: str) -> list[str]:
    errors: list[str] = []
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()

    fm = False
    managed = False
    fence = ""
    size = 0
    start = 0

    def flush() -> None:
        nonlocal size
        if size > 500:
            errors.append(f"{rel_path}:{start}: STD-11 §R11: {size} characters, limit 500")
        size = 0

    for idx, raw in enumerate(lines, start=1):
        line = raw.rstrip("\r")
        if idx == 1 and line == "---":
            fm = True
            continue
        if fm:
            if line == "---":
                fm = False
            continue
        if MANAGED_BEGIN in line:
            flush()
            managed = True
            continue
        if managed:
            if MANAGED_END in line:
                managed = False
            continue

        stripped = line.lstrip(" \t")
        if fence:
            if stripped.startswith(fence) and stripped[len(fence) :].strip(" \t") == "":
                fence = ""
            continue

        if FENCE_OPEN_RE.match(stripped):
            flush()
            ch = stripped[0]
            n = 0
            while n < len(stripped) and stripped[n] == ch:
                n += 1
            fence = ch * n
            continue

        if not line.strip(" \t") or HEADING_OR_TABLE_RE.match(line):
            flush()
            continue

        if LIST_ITEM_RE.match(line):
            flush()

        if size == 0:
            start = idx
        else:
            size += 1
        size += len(line)

    flush()
    return errors


def extract_md_target(raw_dest: str) -> str:
    dest = raw_dest.strip()
    if dest.startswith("<"):
        end = dest.find(">")
        if end != -1:
            return dest[1:end]
    return dest.split(None, 1)[0] if dest else ""


def is_within_root(root_abs: str, candidate: Path) -> bool:
    norm = os.path.abspath(os.path.normpath(str(candidate)))
    return norm == root_abs or norm.startswith(root_abs + os.sep)


def check_links_and_paths(root: Path, rel_path: str, text: str) -> list[str]:
    errors: list[str] = []
    file_path = root / rel_path
    file_dir = file_path.parent
    root_abs = os.path.abspath(str(root))

    fence = ""
    for idx, raw in enumerate(text.split("\n"), start=1):
        line = raw.rstrip("\r")
        stripped = line.lstrip(" \t")
        if fence:
            if stripped.startswith(fence) and stripped[len(fence) :].strip(" \t") == "":
                fence = ""
            continue
        if FENCE_OPEN_RE.match(stripped):
            ch = stripped[0]
            n = 0
            while n < len(stripped) and stripped[n] == ch:
                n += 1
            fence = ch * n
            continue

        for m in MD_LINK_RE.finditer(line):
            target = extract_md_target(m.group(1))
            target = target.split("#", 1)[0].split("?", 1)[0]
            if target.startswith("./"):
                target = target[2:]
            if (
                not target
                or target.startswith("/")
                or "://" in target
                or target.startswith("mailto:")
                or target.startswith("$")
            ):
                continue
            resolved = file_dir / target
            if not (is_within_root(root_abs, resolved) and resolved.exists()):
                errors.append(
                    f"{rel_path}:{idx}: STD-11 §R17: relative link '{target}' does not resolve"
                )

        for m in BACKTICK_RE.finditer(line):
            token = m.group(1)
            if not REPO_PATH_RE.match(token):
                continue
            target = token.split("#", 1)[0].split("?", 1)[0]
            if target.startswith("./"):
                target = target[2:]
            if (
                not target
                or target.startswith("/")
                or "://" in target
                or target.startswith("mailto:")
                or target.startswith("$")
            ):
                continue

            if target.startswith("../"):
                rest = target
                climbed_dir = file_dir
                while rest.startswith("../"):
                    rest = rest[3:]
                    climbed_dir = climbed_dir.parent
                first = rest.split("/", 1)[0]
                if not (
                    (climbed_dir / first).is_dir()
                    or (file_dir / first).is_dir()
                    or (root / first).is_dir()
                ):
                    continue
            else:
                first = target.split("/", 1)[0]
                if not ((file_dir / first).is_dir() or (root / first).is_dir()):
                    continue

            local_candidate = file_dir / target
            root_candidate = root / target
            local_ok = is_within_root(root_abs, local_candidate) and local_candidate.exists()
            root_ok = is_within_root(root_abs, root_candidate) and root_candidate.exists()
            if not (local_ok or root_ok):
                errors.append(
                    f"{rel_path}:{idx}: STD-11 §R17: path '{target}' does not resolve"
                )

    return errors


def check_repository(root: Path) -> list[str]:
    errors: list[str] = []
    if not root.is_dir():
        return [f"{root}: root path is not a directory"]

    files = discover_instruction_files(root)
    if not files:
        return [f"{root}: STD-11 §R20: no instruction files found"]

    regular_agents: list[str] = []

    for rel in files:
        full = root / rel
        name = Path(rel).name
        parent = full.parent
        rel_dir = Path(rel).parent.as_posix()
        source = parent / "AGENTS.md"
        rel_source = "AGENTS.md" if rel_dir == "." else f"{rel_dir}/AGENTS.md"

        if name == "AGENTS.md":
            if full.is_symlink():
                errors.append(
                    f"{rel}: STD-11 §R20: AGENTS.md must be a regular file, not a symlink"
                )
            elif not full.is_file():
                errors.append(
                    f"{rel}: STD-11 §R20: AGENTS.md must be a regular file"
                )
            else:
                regular_agents.append(rel)
        else:
            if source.is_symlink() or not source.is_file():
                errors.append(
                    f"{rel}: STD-11 §R20: missing regular source file {rel_source}"
                )
            elif full.is_symlink():
                if not (full.exists() and os.path.samefile(full, source)):
                    errors.append(
                        f"{rel}: STD-11 §R20: symlink must resolve to {rel_source}"
                    )
            elif not full.is_file():
                errors.append(
                    f"{rel}: STD-11 §R20: provider file must be a regular file or symlink to {rel_source}"
                )
            elif read_first_line(full) != "@AGENTS.md":
                errors.append(
                    f"{rel}: STD-11 §R20: provider file must be a symlink to AGENTS.md or start with '@AGENTS.md'"
                )

        if not full.is_symlink() and full.is_file():
            try:
                text = full.read_text(encoding="utf-8")
            except UnicodeDecodeError as exc:
                errors.append(f"{rel}: invalid UTF-8 ({exc})")
                continue
            errors.extend(check_paragraph_lengths(rel, text))
            errors.extend(check_links_and_paths(root, rel, text))

    for rel in regular_agents:
        full = root / rel
        parent = full.parent
        rel_dir = Path(rel).parent.as_posix()
        limit = 8000 if rel_dir == "." else 4000
        agents_bytes = full.stat().st_size
        if agents_bytes > limit:
            errors.append(
                f"{rel}: STD-11 §R16/§R25: {agents_bytes} bytes, limit {limit}"
            )
        for provider_name in ("CLAUDE.md", "GEMINI.md"):
            provider = parent / provider_name
            if (
                os.path.lexists(provider)
                and not provider.is_symlink()
                and provider.is_file()
                and read_first_line(provider) == "@AGENTS.md"
            ):
                rel_provider = provider_name if rel_dir == "." else f"{rel_dir}/{provider_name}"
                total = agents_bytes + provider.stat().st_size
                if total > limit:
                    errors.append(
                        f"{rel} + {rel_provider}: STD-11 §R16/§R25: {total} bytes, limit {limit}"
                    )

    return errors


def write_tree(root: Path, files: dict[str, str | tuple[str, str]]) -> None:
    for rel, spec in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(spec, tuple):
            kind, target = spec
            assert kind == "symlink"
            os.symlink(target, path)
        else:
            path.write_text(spec, encoding="utf-8")


def run_self_tests() -> None:
    def assert_pass(name: str, files: dict[str, str | tuple[str, str]]) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_tree(root, files)
            errs = check_repository(root)
            assert not errs, f"positive fixture '{name}' unexpectedly failed: {errs}"

    def assert_fail(
        name: str, expected_fragment: str, files: dict[str, str | tuple[str, str]]
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_tree(root, files)
            errs = check_repository(root)
            assert errs, f"negative fixture '{name}' unexpectedly passed"
            assert any(expected_fragment in e for e in errs), (
                f"negative fixture '{name}' failed with {errs}, expected '{expected_fragment}'"
            )

    # ---- Positive fixtures -------------------------------------------------

    # §R11: 500-char multi-byte UTF-8 bullet, consecutive <=500 bullets,
    # and >500 lines inside code fences, tables, and managed blocks.
    utf8_500_chars = "- " + ("é" * 498)
    assert len(utf8_500_chars) == 500 and len(utf8_500_chars.encode("utf-8")) > 500
    assert_pass(
        "R11-valid-paragraphs-fences-tables-managed",
        {
            "AGENTS.md": "\n".join(
                [
                    "# Repo",
                    "",
                    "Short intro.",
                    "",
                    utf8_500_chars,
                    "- " + ("a" * 300),
                    "- " + ("b" * 300),
                    "",
                    "```sh",
                    "echo " + ("x" * 600),
                    "```",
                    "",
                    "| Col | Val |",
                    "|---|---|",
                    "| k | " + ("y" * 600) + " |",
                    "",
                    MANAGED_BEGIN,
                    "Managed " + ("z" * 600),
                    MANAGED_END,
                    "",
                ]
            ),
            "CLAUDE.md": "@AGENTS.md\n",
        },
    )

    # §R17: valid relative links, fragments/queries, backticked paths, external URLs,
    # non-path slash tokens, and ignored fenced examples.
    assert_pass(
        "R17-valid-links-and-paths",
        {
            "docs/guide.md": "# Guide\n",
            "AGENTS.md": "\n".join(
                [
                    "# Repo",
                    "",
                    "See [guide](docs/guide.md#intro) and [external](https://example.com/x) or `#local`.",
                    "Check `docs/guide.md` and `docs/` and `application/json`.",
                    "",
                    "```markdown",
                    "[broken-in-fence](missing.md) `docs/missing.md`",
                    "```",
                    "",
                ]
            ),
            "sub/src/lib.rs": "// lib\n",
            "sub/AGENTS.md": (
                "# Sub\n\nSee [guide](../docs/guide.md), `src/lib.rs`, `src/`, and `docs/guide.md`.\n"
            ),
        },
    )

    # §R20: regular AGENTS.md with @AGENTS.md import (with provider rule) and symlinked GEMINI.md.
    assert_pass(
        "R20-valid-import-and-symlink",
        {
            "AGENTS.md": "# Repo\n\nShort intro.\n",
            "CLAUDE.md": "@AGENTS.md\n\n- Keep Claude output concise.\n",
            "GEMINI.md": ("symlink", "AGENTS.md"),
            "pkg/AGENTS.md": "# Pkg\n\nShort intro.\n",
            "pkg/CLAUDE.md": ("symlink", "AGENTS.md"),
        },
    )

    # §§R16/R25: exact boundary sizes (8,000 root combined, 4,000 subdir combined,
    # and 8,000 root with symlinked provider).
    root_import = "@AGENTS.md\n"  # 11 bytes
    root_lines = ["# R", ""] + ["- " + ("a" * 399) for _ in range(19)]
    root_prefix = "\n".join(root_lines) + "\n- "
    root_pad = 8000 - len(root_import.encode()) - len(root_prefix.encode()) - 1
    assert 0 < root_pad <= 490
    root_agents = root_prefix + ("a" * root_pad) + "\n"
    assert len(root_agents.encode()) + len(root_import.encode()) == 8000

    sub_import = "@AGENTS.md\n"  # 11 bytes
    sub_lines = ["# S", ""] + ["- " + ("b" * 399) for _ in range(9)]
    sub_prefix = "\n".join(sub_lines) + "\n- "
    sub_pad = 4000 - len(sub_import.encode()) - len(sub_prefix.encode()) - 1
    assert 0 < sub_pad <= 490
    sub_agents = sub_prefix + ("b" * sub_pad) + "\n"
    assert len(sub_agents.encode()) + len(sub_import.encode()) == 4000

    assert_pass(
        "R25-exact-byte-budgets",
        {
            "AGENTS.md": root_agents,
            "CLAUDE.md": root_import,
            "GEMINI.md": ("symlink", "AGENTS.md"),
            "sub/AGENTS.md": sub_agents,
            "sub/CLAUDE.md": sub_import,
        },
    )

    # ---- Negative fixtures -------------------------------------------------

    assert_fail(
        "R11-paragraph-over-500-chars",
        "STD-11 §R11",
        {
            "AGENTS.md": "# Repo\n\n" + ("a" * 501) + "\n",
        },
    )
    assert_fail(
        "R11-wrapped-bullet-over-500-chars",
        "STD-11 §R11",
        {
            "AGENTS.md": "# Repo\n\n- " + ("a" * 250) + "\n  " + ("b" * 250) + "\n",
        },
    )
    assert_fail(
        "R11-numbered-step-over-500-chars",
        "STD-11 §R11",
        {
            "AGENTS.md": "# Repo\n\n1. " + ("a" * 498) + "\n",
        },
    )
    assert_fail(
        "R11-import-file-bullet-over-500-chars",
        "STD-11 §R11",
        {
            "AGENTS.md": "# Repo\n\nShort.\n",
            "CLAUDE.md": "@AGENTS.md\n\n- " + ("c" * 499) + "\n",
        },
    )

    assert_fail(
        "R17-broken-markdown-link",
        "STD-11 §R17",
        {
            "AGENTS.md": "# Repo\n\nSee [missing](docs/missing.md).\n",
        },
    )
    assert_fail(
        "R17-broken-subdir-relative-link",
        "STD-11 §R17",
        {
            "AGENTS.md": "# Repo\n",
            "sub/AGENTS.md": "# Sub\n\nSee [missing](../docs/nope.md).\n",
        },
    )
    assert_fail(
        "R17-broken-backticked-path",
        "STD-11 §R17",
        {
            "docs/present.md": "# Present\n",
            "AGENTS.md": "# Repo\n\nCheck `docs/absent.md`.\n",
        },
    )
    assert_fail(
        "R17-broken-link-in-import-file",
        "STD-11 §R17",
        {
            "AGENTS.md": "# Repo\n",
            "CLAUDE.md": "@AGENTS.md\n\nSee [missing](missing.md).\n",
        },
    )

    assert_fail(
        "R20-provider-without-agents",
        "STD-11 §R20",
        {
            "CLAUDE.md": "@AGENTS.md\n",
        },
    )
    assert_fail(
        "R20-provider-without-import-line",
        "STD-11 §R20",
        {
            "AGENTS.md": "# Repo\n",
            "CLAUDE.md": "# Duplicate instructions\n",
        },
    )
    assert_fail(
        "R20-agents-is-symlink",
        "STD-11 §R20",
        {
            "README.md": "# Repo\n",
            "AGENTS.md": ("symlink", "README.md"),
        },
    )
    assert_fail(
        "R20-provider-symlink-wrong-target",
        "STD-11 §R20",
        {
            "AGENTS.md": "# Repo\n",
            "OTHER.md": "# Other\n",
            "GEMINI.md": ("symlink", "OTHER.md"),
        },
    )

    assert_fail(
        "R25-root-agents-over-8000-bytes",
        "STD-11 §R16/§R25",
        {
            "AGENTS.md": "# R\n\n" + "\n\n".join(("a" * 400) for _ in range(20)),
        },
    )
    assert_fail(
        "R25-root-combined-over-8000-bytes",
        "STD-11 §R16/§R25",
        {
            "AGENTS.md": root_agents + "x",
            "CLAUDE.md": root_import,
        },
    )
    assert_fail(
        "R25-subdir-agents-over-4000-bytes",
        "STD-11 §R16/§R25",
        {
            "AGENTS.md": "# Repo\n",
            "sub/AGENTS.md": "# S\n\n" + "\n\n".join(("b" * 400) for _ in range(10)),
        },
    )
    assert_fail(
        "R25-subdir-combined-over-4000-bytes",
        "STD-11 §R16/§R25",
        {
            "AGENTS.md": "# Repo\n",
            "sub/AGENTS.md": sub_agents + "y",
            "sub/GEMINI.md": sub_import,
        },
    )


def main() -> int:
    self_test_flag = sys.argv[1] == "1"
    target = Path(sys.argv[2]).resolve()

    if self_test_flag:
        run_self_tests()

    errors = check_repository(target)
    if errors:
        for err in errors:
            print(f"agent context check: FAIL: {err}", file=sys.stderr)
        return 1

    checked = discover_instruction_files(target)
    suffix = " (self-test passed)" if self_test_flag else ""
    print(f"agent context check: OK ({len(checked)} instruction files verified{suffix})")
    return 0


sys.exit(main())
PY
