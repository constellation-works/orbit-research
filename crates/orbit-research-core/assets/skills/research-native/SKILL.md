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
identical retry. Question revisions require the expected Git blob so concurrent
changes fail safely. Preserve conflicting evidence, failed controls, limitations,
and lineage; do not strengthen a conclusion from task or delivery state.

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

Use the MCP tools or equivalent `orbit-research research` subcommands. Always
select the corpus explicitly.
