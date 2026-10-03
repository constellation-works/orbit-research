# orbit-research

A local research workbench for canonical Observatory Markdown records, with a
CLI and MCP interface. Orbit owns execution tasks, crews, runs and delivery
directly; scientific records stay in the selected knowledgebase.

The workbench also ships as an Orbit plugin (see the constellation
`operations/research/orbit-research-plugin.md` spec). Local corpus operations
(capture, list, show, check, create, revise, assess, planning) are available.
Writes run in primary mode on the primary checkout (allocate IDs and commit) or
in worktree mode inside a linked run worktree (write only the reserved R, never
commit); every write result reports its `mode`. `.orbit-plugin/plugin.yaml`
(plugin root) exposes `list`, `show`, `check`, `version`, `plan` and `validate` as
sandboxed, read-only Orbit plugin tools, `open-questions`, `awaiting-acceptance`,
`hypotheses` and `corpus-health` as the sources of four dashboard panels (below), and
`link`/`accept` as the two mutating tools (`fs.read: {{workspace}}`, a scratch `fs.write` for `accept`'s
own staged artifact, no `unsandboxed` grant, no `requires.programs`), served
by `orbit-research orbit-tool`; see
[the plugin transport note](ARCHITECTURE.md). The plugin also ships the
`research_investigation` job (under `.orbit-plugin/definitions/`): start one investigation
with `orbit run job research_investigation --input task=<task-id>`. The run
works in its own worktree, and the `validate` step fails it before commit when
the written record is invalid. Once delivery lands, `accept`
persists `research-acceptance.json` as a task artifact on the R's task; a retry whose
record, README blob, run id and input digests match returns the stored acceptance
(`recorded: false`, original commit kept) even after later corpus commits.
`research assess` (CLI and MCP) runs outside the plugin sandbox: it reads the R's
`orbit.task`, fetches that artifact by running `orbit tool run` from the corpus
checkout (the Orbit workspace; `orbit` is `$ORBIT_BIN` or the one on `PATH`), and
appends the verdict only when the artifact names the R and the README blob it
accepted is the README at HEAD. It refuses, never accepting by default, when the
R has no task, the artifact is missing, malformed or for another record, the
README changed since acceptance, or Orbit cannot be reached. It also sets the
hypothesis `status` from the owner schema's `verdict_status` for the current
revision: the last assessment appended on that revision wins, an earlier revision's
assessment never changes it, and a `dropped` hypothesis stays dropped.
`link` takes `plan`'s output as is: add `research_id` and `request_key` to the
plan's JSON and pass it, `context_files` included. The task is scoped to that
field: the R directory for an investigation, one unit's `code/<unit>/` and
`artifacts/<unit>/` for a contribution, the README and manifest for a synthesis.
`link` accepts only a value `plan` derives for that R (any other path, another
unit, or a widened list is refused) and omitting it scopes the task to the R
directory. A request key stays bound to its first scope: the same key with a
different scope is a conflict, not an adoption. An accepted result is not
re-accepted with different evidence; to revise it, reserve a new R derived from it.
The plugin's `orbit-research-native` skill (`.orbit-plugin/skills/native/`) walks the whole loop.

### Dashboard panels

In Orbit's dashboard Plugins tab, a workspace with the plugin enabled shows four
read-only panels, drawn by Orbit's generic renderer (no plugin code):

| Panel | Render | Shows |
|---|---|---|
| Open questions | table | each open question with its tags and the Orbit tasks recorded on results working on it |
| Results awaiting acceptance | table | results a run delivered (committed with `orbit.task` and `orbit.run`) that are not accepted: no `research-acceptance.json` on the task (`awaiting acceptance`), one for an earlier version of the README (`result changed since accepted`), or an acceptance that cannot be checked right now (`acceptance unknown`, with a short reason such as `orbit not available`, `orbit call timed out` or `artifact unreadable`) |
| Hypotheses and assessments | table | each hypothesis revision with the latest verdict from each result; results that disagree stay on separate rows and the revision is marked disputed |
| Corpus health | kv | validity, base revision and counts by kind and status |

A workspace with no corpus, an unreadable corpus or nothing to list shows one short
sentence saying so and what to do next, not an error. A research corpus needs its own
new, empty directory (`orbit-research workspace init <dir>`) registered as an Orbit
workspace; the plugin reads the workspace root as the corpus, so an existing project
directory will not do. A table shows at most 200 rows and ends with a row saying how
many were left out and the `orbit-research research list --corpus <path>` command that
lists them. Panels never write a research record; run `accept` and `assess` from the
CLI or MCP (see the skill).

`awaiting-acceptance` calls `orbit tool run` for each delivered result, killing any one
call after 5 seconds and stopping all of them after 20, so the panel always answers. A
result is accepted only when its task's artifact reads as the full acceptance (research
id, commit, blob and run id) and names that result and its current README, the same
check `assess` applies; a partial or malformed artifact is `acceptance unknown`
(`artifact unreadable`). A task that cannot be read is never assumed accepted. The one
exception is the acceptance cache: after a live read confirms an acceptance, the panel
keeps a small file for that (research id, task, README blob) under the workspace's
`_data/orbit-research-operations/acceptance/` (only in a corpus prepared by
`workspace init` or `workspace prepare-operations`), and a later refresh hides that row
without calling Orbit, even while Orbit is unreachable. That is safe by design: a stored
acceptance is never replaced and an edited README no longer matches the key. The cache is
trusted local state under the plugin's own write grant, like the corpus itself: someone
who can write that directory can plant an entry that hides a row. It can only hide a row,
never add one, and `assess` never reads it.

## Build and validate

Requires Rust 1.89+ and Git.

```sh
cargo build --workspace --locked
make test
```

`make install` builds a release CLI and installs it to `~/.local/bin`. Override
`INSTALL_BIN_DIR` for another destination or use `INSTALL_PROFILE=debug` for a
development build. `CARGO_TARGET_DIR` is respected. `BUILD_BUDGET` optionally names
a command wrapper accepting `-- COMMAND ...`; its default (`env`) runs Cargo directly.

The source plugin root is `.orbit-plugin/`. Before `orbit plugin add .`, run
`cargo build --release -p orbit-research-cli --locked`, then
`scripts/bundle-plugin-binary.sh --binary target/release/orbit-research`.
The script copies the executable into `.orbit-plugin/bin/orbit-research.bin`; that ignored
binary travels with the plugin root when Orbit installs it. The launcher uses
only that adjacent binary. Regenerate request schemas with
`scripts/generate-plugin-schemas.sh`; the Cargo drift checks read the generated
files in `.orbit-plugin/schemas/`.

The v1 loop is proven end to end by `tests/e2e_v1_loop.rs` (CI's installed-Orbit job, a
scripted agent) and exercised once with a real crew by `scripts/e2e-live.sh`, which is guarded
by `--live` and `ORBIT_RESEARCH_LIVE_CONFIRM=yes`; see
[`docs/plugin-validation.md`](docs/plugin-validation.md).

## Design and structure

- [Architecture](ARCHITECTURE.md): the four crates and module ownership.
- [Workbench design](docs/design/research-workbench/1_overview.md): product scope
  (superseded in part; see the note at its top).
- [Research contracts](docs/design/research-workbench/specs/contracts.md): canonical
  records and owner boundaries (execution sections superseded; see the note at
  its top).
- [Dashboard design](docs/design/user-interface/1_overview.md): superseded — the
  standalone dashboard is removed; research views are the four Orbit plugin panels above.
- [CLI design](docs/design/terminal-interface/1_overview.md).

The supported workflow uses canonical Observatory Markdown records. Use the CLI
or MCP server against an explicitly selected corpus; Orbit owns task creation,
dispatch and delivery through its own native commands.

## Terminal use

Run `orbit-research` or `orbit-research --help` for readable help and examples.
Output defaults to human tables on a terminal and headerless TSV lists in pipes.
Scripts that previously relied on default JSON must select `--format json` or set
`ORBIT_RESEARCH_FORMAT=json`; explicit JSON retains the canonical result shape.

```sh
orbit-research research list --corpus ./observatory
orbit-research research show --corpus ./observatory --id Q001
orbit-research --format json research list --corpus ./observatory
```

Use `workspace init PATH` to create a new corpus (a Git repository on branch `main`,
whatever `init.defaultBranch` says, which is the `research_investigation` job's default
`base_branch`; pass `--input base_branch=<branch>` to the job for an existing corpus on
another branch) or validate an existing one.
On Linux and macOS, new corpora include the local operational state needed for
research request correlations. To prepare an existing corpus explicitly, run
`orbit-research workspace prepare-operations PATH` against its primary checkout.
Preparation leaves canonical research records and Git history unchanged; repeat
calls report `changed: false`. It also recreates a deleted
`_data/orbit-research-operations/` (the old link records are gone) and makes Git ignore the
plugin's `.orbit-research-tmp/` scratch directory through the repository's local exclude file. Existing-corpus `workspace init` remains
validation only.

`research plan` drafts work without creating a task or changing the corpus.
Select `--shape investigation` with `--objective`, `--shape contribution` with
`--objective` and `--unit`, or `--shape synthesis` with at least one
`--contribution` (repeat for several). Flags for another shape are rejected as
usage errors rather than discarded.

See the [terminal design](docs/design/terminal-interface/2_design.md) for output,
error and compatibility contracts.
