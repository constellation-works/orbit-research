---
name: orbit-research-native
description: Run the research loop on a canonical Q/H/T/R corpus with the orbit-research plugin - capture a question, plan and reserve an investigation, link it to an Orbit task, run research_investigation, validate, accept the delivery and assess the hypothesis honestly. Covers the two writer modes, the scientific safeguards, the operator requirement for mutating plugin tools and preparing an existing corpus.
---

# Native research workflow

Canonical questions (Q), hypotheses (H), theories (T) and research results (R)
live as Markdown in the corpus, which is the Orbit workspace the plugin is
enabled in. The corpus's `_scripts/schema.json` is the write contract. Orbit
owns tasks, runs and delivery. The plugin reads the corpus and links, validates
and accepts work; the `orbit-research` binary writes records. Execution success
never establishes scientific support.

## The loop

1. **Capture.** `orbit-research research capture --corpus <path> --text "<question>" --tag <tag>`
   records a Q from text and tags alone, with no task. Reuse `--request-key` on
   an identical retry. Read what exists first with
   `orbit-research research check --corpus <path>`,
   `orbit-research research list --corpus <path>` and
   `orbit-research research show --corpus <path> --id <id>` (or the plugin's `check`,
   `list` and `show`). The `research` subcommands below all take `--corpus <path>`.
2. **Plan.** `research plan --shape investigation --research-id <R> --objective "<outcome>"`
   drafts an Orbit task (title, description, acceptance criteria, context files).
   It creates nothing. Use `--shape contribution --unit <name>` for disjoint
   `code/<unit>/` and `artifacts/<unit>/` paths, and `--shape synthesis
   --contribution <name>` to reconcile completed contributions.
3. **Reserve.** On the primary checkout,
   `research create --kind R --status planned --title "<title>" --request-key <key> --derived-from <Q or H>`
   allocates the next R id and commits the stub before anything is dispatched.
   Only primary-mode `create` allocates ids.
4. **Link.** The plugin's `link` tool creates the Orbit task from the plan,
   tagged `research-request:<key>` with context files naming the reserved R.
   An identical retry adopts the one task already tagged; it never creates a
   second. Pass the plan's text, criteria and context unchanged.
5. **Run.** `orbit run job research_investigation --input task=<task-id>`. The job
   works in its own worktree: the `research_investigate` agent writes the
   reserved R, `validate` gates it, then Orbit commits, merges and moves the task to
   `review`. Orbit owns dispatch, status and cancellation (`orbit run show`,
   `orbit task update`, `orbit run cancel`).
6. **Validate.** The job's `validate` step fails the run before commit when a
   README section (`Question`, `Method`, `Result`, `Limitations`, `Next`) is missing
   or still a placeholder, `orbit.task`/`orbit.run` name another run, a local input
   no longer matches its manifest digest, a reference dangles, or the worktree
   allocated an id or changed any other record. Fix the cause and rerun; do not
   edit around the gate.
7. **Accept.** Once the task is in `review` or `done`, the plugin's `accept` tool
   (`task_id`, `research_id`) re-validates the published commit and stores
   `research-acceptance.json` (record, blob, commit, input digests, run id) on the
   task. A retry whose evidence matches (record, README blob, run id and input
   digests) is idempotent whatever the current HEAD: it returns the stored
   acceptance with `recorded: false` and never rewrites its commit. It refuses
   before delivery lands, for a failed run, on a validation finding and on a
   stored-evidence mismatch, which names the differing field.
8. **Assess.** `research assess --id <H> --expected-blob <blob> --research <R>
   --revision <n> --verdict supports|refutes|inconclusive --strength
   anecdote|suggestive|strong` appends one explicit verdict to the hypothesis. It runs
   outside the plugin sandbox, finds the R's `orbit.task`, fetches that task's
   `research-acceptance.json` through Orbit and refuses unless the artifact names that
   R and the README blob it accepted is the README at HEAD. If the README changed
   after `accept`, validate and `accept` again; if Orbit is unreachable, fix
   `ORBIT_BIN`/`PATH` and retry. It needs an existing revision and only appends. You
   state the verdict; acceptance never supplies or strengthens one.

## Two writer modes

Every write reports its `mode`; `--mode primary|worktree` makes a mismatch refuse.

- **Primary checkout** (a person): takes the exclusive checkout lock, allocates ids
  and commits. Used for `capture`, `create`, `revise` of Q/H/T, `revise-question`
  and `assess`. Revisions and assessments need `--expected-blob` (the `git_blob`
  from `show`), so a concurrent change refuses instead of being overwritten.
- **Run worktree** (the investigation agent): writes only the reserved R's README
  and `data/manifest.json`, uncommitted, with
  `research revise --mode worktree --id <R> --expected-blob <blob> --status done
  --orbit-task <task> --orbit-run <run> --body-file <body.md> --manifest-file <manifest.json>`.
  It never allocates an id, never commits and refuses any other record. The job's
  commit step publishes the files.

A changed hypothesis statement bumps its revision. Earlier assessments stay on the
revision they judged.

## Scientific safeguards

- Execution success is not support. A run that finishes, a script that exits 0 or
  a plot that renders is not evidence for the hypothesis. Report what the evidence
  shows, including evidence against it.
- Failed controls make the result inconclusive. If a control, baseline or sanity
  check fails, say so in Result and Limitations, claim neither support nor
  refutation, and assess it `inconclusive`. Such a record is still valid and
  deliverable.
- Never edit H or T from an investigation, and never append an assessment there.
  Verdicts are a human step after acceptance.
- Preserve conflicting evidence, limitations and lineage. When results disagree on
  a revision, keep every assessment; the dashboard shows them as separate rows.

## Operator requirement

The plugin's mutating tools, `link` and `accept`, are refused for callers without
operator capability, and an agent inside a run is not one. From a local shell, set
`ORBIT_OPERATOR=1` for that one command, Orbit's explicit and audited override:

```sh
ORBIT_OPERATOR=1 orbit tool run orbit.research.link --input '{"research_id":"R001","request_key":"<key>","title":"<title>"}'
ORBIT_OPERATOR=1 orbit tool run orbit.research.accept --input '{"task_id":"<task>","research_id":"R001"}'
```

Over MCP, start the session as an operator (`orbit mcp serve --operator`). The
namespace is `orbit.research` for the verified install and `research` for a local
development install. The read-only tools (`list`, `show`, `check`, `plan`,
`version`, `validate` and the panel sources) need no override.

## Existing corpora

A corpus made by `orbit-research workspace init` is ready. For an older one,
`link` refuses before creating any task until its owner prepares shared request
storage once, on the primary checkout:

```sh
orbit-research workspace prepare-operations /absolute/path/to/corpus
```

It leaves records and Git history unchanged, and repeating it reports
`changed: false`.

## Dashboard panels

The plugin adds four read-only panels to Orbit's Plugins tab for the workspace:
open questions with their tags and linked tasks; results awaiting acceptance (delivered
by a run, no acceptance artifact yet; `acceptance unknown` when the task cannot be
read, never assumed accepted); hypotheses with the latest verdict per result and
revision, disagreements on separate rows; and corpus health. Use them to see what
needs `accept` or `assess` next. A workspace without a corpus shows a short
explanation instead.

Always select the corpus explicitly with `--corpus`, and use `--json` when a script
reads the result.
