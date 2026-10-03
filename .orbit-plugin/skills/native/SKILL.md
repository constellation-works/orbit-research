---
name: orbit-research-native
description: Run the research loop on a canonical Q/H/T/R corpus with the orbit-research plugin - create the corpus, capture a question, state a hypothesis, reserve the result, plan and link its Orbit task, run research_investigation, validate, accept the delivery and assess the hypothesis honestly. Covers the two writer modes, the scientific safeguards, the operator requirement for mutating plugin tools and preparing an existing corpus.
---

# Native research workflow

Canonical questions (Q), hypotheses (H), theories (T) and research results (R)
live as Markdown in the corpus, which is the Orbit workspace the plugin is
enabled in. The corpus's `_scripts/schema.json` is the write contract. Orbit
owns tasks, runs and delivery. The plugin reads the corpus and links, validates
and accepts work; the `orbit-research` binary writes records. Execution success
never establishes scientific support.

## The loop

The order matters: a result must be reserved before it can be planned, and
planned before it can be linked. The `sh` blocks are meant to be pasted in
order into one shell. The ids shown (`Q001`, `H001`, `R001`) are what a fresh
corpus allocates; elsewhere use the ids each command prints. Writes commit, so
Git needs a `user.name` and `user.email`. Blocks marked `sh orbit` call Orbit
and need the plugin installed and enabled in the corpus's workspace (an owner
step); the others need only `orbit-research`.

1. **Create the corpus and look around.** `workspace init` makes a Git repository
   on branch `main` whatever Git's `init.defaultBranch` says, with the owner schema
   and the shared request storage `link` needs. Read what exists with `check`,
   `list` and `show` (the plugin has the same three). Every `research` subcommand
   takes `--corpus <path>`.

```sh
corpus="$PWD/research-corpus"
orbit-research workspace init "$corpus"
orbit-research research check --corpus "$corpus"
orbit-research research list --corpus "$corpus"
```

2. **Capture.** `capture` records a Q from text and tags alone, with no task.
   Reuse `--request-key` on an identical retry.

```sh
orbit-research research capture --corpus "$corpus" --text "Does the baseline hold under load?" --tag perf
```

3. **State the hypothesis.** Create the H the investigation will test, derived
   from the question. Its claim is the `--body`; it starts `open` at revision 1.

```sh
orbit-research research create --corpus "$corpus" --kind H --title "The baseline holds under load" --body "Latency stays within 10 percent of the idle baseline at 2x load." --derived-from Q001 --request-key hypothesis-1
```

4. **Reserve the result.** On the primary checkout `create --kind R --status planned`
   allocates the next R id and commits the stub before anything is dispatched.
   Only primary-mode `create` allocates ids, and `plan` and `link` refuse an R
   that was never reserved.

```sh
orbit-research research create --corpus "$corpus" --kind R --status planned --title "Load test of the baseline" --derived-from Q001 --derived-from H001 --request-key reserve-1
```

5. **Plan.** `plan` drafts an Orbit task (title, description, acceptance
   criteria, `context_files`) for the reserved R and creates nothing.
   `--shape investigation` needs `--objective`. `--shape contribution` needs
   `--unit <name>` and `--objective`, and gives disjoint `code/<unit>/` and
   `artifacts/<unit>/` paths. `--shape synthesis` needs `--contribution <name>`
   (repeat it) and reconciles completed contributions.

```sh
orbit-research --json research plan --corpus "$corpus" --shape investigation --research-id R001 --objective "Measure p99 latency at idle and at 2x load, with a control run" | tee plan.json
```

6. **Link.** The plugin's `link` tool creates the Orbit task from the plan,
   tagged `research-request:<key>`. Its input is the plan's JSON object plus
   `research_id` and `request_key`, so pass the plan straight through,
   `context_files` included. `link` derives `context_files` itself, the reserved
   R's directory, and refuses a different value with a message naming both: it
   never widens or changes a task's scope. Only the investigation plan matches;
   a contribution or synthesis plan names narrower paths, so delete its
   `context_files` before linking (the task then covers the R directory, and its
   criteria carry the limits). An identical retry adopts the one task already
   tagged; it never creates a second. Mutating plugin tools need the operator
   override described below.

```sh orbit
ORBIT_OPERATOR=1 orbit tool run orbit.research.link --input "$(jq --arg id R001 --arg key link-R001 '. + {research_id: $id, request_key: $key}' plan.json)"
```

7. **Run.** The job works in its own worktree: the `research_investigate` agent
   writes the reserved R, `validate` gates it, then Orbit commits, merges and moves
   the task to `review`. Orbit owns dispatch, status and cancellation
   (`orbit run show`, `orbit task update`, `orbit run cancel`). The job's
   `base_branch` input defaults to `main`, the branch `workspace init` creates;
   for an existing corpus on another branch add `--input base_branch=<branch>`
   to the command. Run it from the corpus checkout, or add `--workspace <name>`,
   and replace `<task-id>` with the `task_id` that `link` printed.

```sh orbit
orbit run job research_investigation --input task=<task-id>
```

8. **Validate.** The job's `validate` step fails the run before commit when a
   README section (`Question`, `Method`, `Result`, `Limitations`, `Next`) is missing
   or still a placeholder, `orbit.task`/`orbit.run` name another run, a local input
   no longer matches its manifest digest, a reference dangles, or the worktree
   allocated an id or changed any other record. Fix the cause and rerun; do not
   edit around the gate.

9. **Accept.** Once the task is in `review` or `done`, the plugin's `accept` tool
   (`task_id`, `research_id`) re-validates the published commit and stores
   `research-acceptance.json` (record, blob, commit, input digests, run id) on the
   task. A retry whose evidence matches (record, README blob, run id and input
   digests) is idempotent whatever the current HEAD: it returns the stored
   acceptance with `recorded: false` and never rewrites its commit. It refuses
   before delivery lands, for a failed run, on a validation finding and on a
   stored-evidence mismatch, which names the differing field (for a changed
   README, `blob`). An acceptance is never replaced. If the README changes after
   `accept`, a second `accept` conflicts on `blob` and `assess` refuses the stale
   blob, so do not edit an accepted README. To revise accepted work, reserve a new
   R derived from the old one (`--derived-from R001`), then plan, link, run and
   accept that one, and assess the hypothesis against the new R.

```sh orbit
ORBIT_OPERATOR=1 orbit tool run orbit.research.accept --input '{"task_id":"<task-id>","research_id":"R001"}'
```

10. **Assess.** `assess` appends one explicit verdict to the hypothesis. It runs
    outside the plugin sandbox, finds the R's `orbit.task`, fetches that task's
    `research-acceptance.json` through Orbit and refuses unless the artifact names
    that R and the README blob it accepted is the README at HEAD. If Orbit is
    unreachable, fix `ORBIT_BIN`/`PATH` and retry. It needs an existing revision and
    only appends. You state the verdict; acceptance never supplies or strengthens
    one. `assess` also moves the hypothesis's `status`, by the owner schema's
    `verdict_status` map (the bundled schema maps `supports` to `supported`,
    `refutes` to `refuted` and `inconclusive` to `inconclusive`). Only an
    assessment of the hypothesis's current revision does, and the last one appended
    on that revision decides, so an `inconclusive` after a `supports` leaves the
    status `inconclusive`. Every assessment stays recorded; one on an earlier
    revision never changes the status, and a `dropped` hypothesis stays dropped.

```sh orbit
blob=$(orbit-research --json research show --corpus "$corpus" --id H001 | jq -r .git_blob)
revision=$(orbit-research --json research show --corpus "$corpus" --id H001 | jq -r .metadata.revision)
orbit-research research assess --corpus "$corpus" --id H001 --expected-blob "$blob" --research R001 --revision "$revision" --verdict inconclusive --strength anecdote --note "Control run failed; no claim either way"
```

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

```sh orbit
ORBIT_OPERATOR=1 orbit tool run orbit.research.link --input '{"research_id":"R001","request_key":"link-R001","title":"Investigate R001"}'
ORBIT_OPERATOR=1 orbit tool run orbit.research.accept --input '{"task_id":"<task-id>","research_id":"R001"}'
```

The loop above passes the plan's whole output to `link` instead of just a title.

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
