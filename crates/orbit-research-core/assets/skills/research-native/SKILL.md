---
name: orbit-research-native
description: Maintain canonical Observatory Markdown research records and plan explicitly linked Orbit work.
---

# Native research workflow

Canonical questions, hypotheses, theories, and research items live as Observatory
Markdown in the explicitly selected corpus. Orbit owns execution tasks, crews,
runs, and delivery. Execution success does not establish scientific support.

Start by checking the corpus and reading its current records:

```sh
orbit-research research check --corpus /path/to/corpus
orbit-research research list --corpus /path/to/corpus
```

`research check` validates the corpus and reports success, its base revision,
and record and tag counts without including record bodies. Use `research list`
to browse the canonical snapshot and `research show` to read one record. For
scripts, `--format json` or `--format ndjson` keeps the check result structured.

Create a record with a stable request key. Reuse the same request key for an
identical retry. `research capture` records a question from text and tags alone.
Revisions and assessments require the expected Git blob so concurrent changes
fail safely.

Writes run in one of two modes, reported as `mode` in every result. On the
primary checkout (primary mode) the writer allocates IDs and commits; reserve an
investigation with `research create --kind R --status planned` before dispatch.
Inside a run worktree (worktree mode) use `research revise` only on the reserved
R: it writes the README and `data/manifest.json` uncommitted, and allocation or
any other record refuses. Pass `--mode` to refuse when the checkout is not the
mode you expect.

`research assess` appends an explicit verdict to a hypothesis against an existing
revision and an accepted research result: it finds the R's `orbit.task`, fetches
that task's `research-acceptance.json` through Orbit, and refuses unless it names
that R and the README as it is now. If the README changed after `accept`, run
validation and `accept` again; if Orbit is unreachable, fix `ORBIT_BIN`/`PATH` and
retry. A changed hypothesis statement gets a
new revision; earlier verdicts stay on theirs. Failed controls are
`inconclusive`, never `supports`. Preserve conflicting evidence, failed controls,
limitations, and lineage; do not strengthen a conclusion from task or delivery
state.

Plan work before linking it to Orbit. Investigation plans own one research item.
Contribution plans assign disjoint `code/<unit>/` and `artifacts/<unit>/` paths.
Synthesis plans reconcile completed contributions into the shared research README
and input manifest. Use the returned context files and instructions unchanged when
linking work.

Orbit owns task creation, dispatch, status and cancellation directly through its
own native commands (`orbit run job`, `orbit run show`, `orbit task update`,
`orbit run cancel`); this app no longer shells out to an Orbit CLI adapter to
drive them. Use the returned plan's context files and instructions unchanged
when creating the Orbit task.

Run an investigation with `orbit run job research_investigation --input
task=<task-id>`. Its agent writes the reserved R in worktree mode, recording the
run's task and run IDs with `--orbit-task` and `--orbit-run`. The plugin's
`validate` step then fails the run before commit if a README section is missing
or still a placeholder, the provenance names another run, a local input no
longer matches its manifest digest, a reference is dangling, or the worktree
allocated an ID or changed any other record.

Use the MCP tools or equivalent `orbit-research research` subcommands. Always
select the corpus explicitly.
