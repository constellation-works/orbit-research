# `accept` and `assess` in detail

## `accept`

`accept` takes `task_id` and `research_id`. It reads the task's status and run from
Orbit, re-runs the `validate` checks on the published commit of the corpus, and stores
`research-acceptance.json` on the task: record id, README blob, commit, input digests
and run id.

It refuses:

- before delivery lands: the task is not `review` or `done`;
- a task with no recorded run, which is every task that no job ran;
- any `validate` finding, including an R whose `orbit.task` and `orbit.run` do not
  name that task and its run;
- stored evidence that differs from what it would record, naming the differing field
  (`research_id`, `blob`, `run_id` or `artifact_digests`).

A retry whose evidence matches is idempotent whatever the current HEAD: it returns the
stored acceptance with `recorded: false` and never rewrites its commit. An acceptance
is never replaced.

## Revising accepted work

If the README changes after `accept`, a second `accept` conflicts on `blob` and
`assess` refuses the stale blob. Do not edit an accepted README. Reserve a new R
derived from the old one (`--derived-from R001`), then plan, link, run and accept that
one, and assess the hypothesis against the new R.

## `assess`

`assess` appends one explicit verdict (`supports`, `refutes` or `inconclusive`, with a
strength of `anecdote`, `suggestive` or `strong`) to a hypothesis. It runs on the
primary checkout, outside the plugin sandbox, and only appends.

It finds the R's `orbit.task`, fetches that task's `research-acceptance.json` through
Orbit and refuses when:

- the R has no `orbit.task`, or the task has no acceptance: run `accept` first;
- the artifact names another R: the R's `orbit.task` points at the wrong task;
- the README at HEAD is not the blob that was accepted: reserve a new R, as above;
- Orbit cannot be reached: `assess` runs `orbit` from the corpus checkout, so fix
  `ORBIT_BIN` or `PATH`, start from the corpus checkout and retry;
- `--revision` is not a revision the hypothesis has, or `--expected-blob` is stale.

## Hypothesis status

`assess` also moves the hypothesis's `status`, by the owner schema's `verdict_status`
map. The bundled schema maps `supports` to `supported`, `refutes` to `refuted` and
`inconclusive` to `inconclusive`.

- Only an assessment of the hypothesis's current revision moves the status.
- The last one appended on that revision decides, so an `inconclusive` after a
  `supports` leaves the status `inconclusive`.
- An assessment of an earlier revision never changes the status.
- A `dropped` hypothesis stays dropped.

Every assessment stays recorded.
