---
type: design
summary: "Spec: JSON CLI compatibility with Python orbit-research 0.3"
last_validated: 2026-09-12
---

# Spec: CLI Compatibility

The Rust binary is named `orbit-research`. Callers that pass the Python
0.3 argv and read JSON from stdout/stderr must not need a flag translation
layer. `operations/scripts/research-view.sh` is the first such caller.

## Why This Exists

The constellation launcher pins two Git revisions of this repository and
invokes the installed console script. A renamed binary or restructured
subcommand would break that pin independently of digest correctness.

## Commands

Required subcommands, matching `src/orbit_research/cli.py`:

- `index --config PATH --database PATH`
- `browse-export --config PATH --database PATH --output PATH`
- `index-trace --database PATH --key KEY`
- `resource --version 1`
- `task-context --orbit-root --host --workspace --task --run [--orbit-executable]`
- `validate PATH [--target PATH ...]`
- `import ADAPTER --source-root PATH --repository NAME [--expect-revision REV] [--select REL] [--dry-run] [--output PATH]`
- `reconcile PATH --output PATH [--target PATH ...]`
- owner ops: `program`, `claim`, `artifact`, `preregister`, `begin-run`,
  `record-run`, `assess`, `retire`, `heads`, `ref`, `trace`, `export`
  with `--owner-root`, `--repository`, optional `--records`, `--source`,
  and the per-command `--request` / `--id` / `--revision` /
  `--source-revision` / `--output` flags as today

`import` is dry-run whether or not `--dry-run` is passed. There is no
write mode.

## Output

- Success and validation results: one JSON document on stdout,
  `ensure_ascii=False` equivalent, no NaN.
- Failures: `{"error":{"code":"<code>","message":"<text>"}}` on stderr.
  `index` invalid owner documents may also include `problems`.
- Exit `0` on success, `1` when `validate` reports errors, `2` on
  invalid-input (including `IndexBuildError`).

Do not add human tables as the default. A later `--format` is allowed only
if `json` remains the default for existing subcommands.

## Failure modes

- Inferring `--orbit-root` or `--repository` from cwd: forbidden.
- Writing reports or exports inside the source/owner root: forbidden.
- Replacing a previous `browse-export` destination in place: forbidden;
  the destination must be new.
- Serving HTTP from this binary: forbidden.

## Migration

While Python remains, both CLIs must accept the same argv for the commands
the research-view launcher uses (`index`, `browse-export`). After cutover,
the Python console script is removed in the same change that updates the
pins.

## Agent Signature

grok · 2026-09-12
