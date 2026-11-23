---
name: research-loop
description: Records experiments as questions, hypotheses, theories and results in an orbit-research corpus and keeps their verdicts honest. Use when about to run an experiment, measurement, benchmark or comparison whose result should be kept; when stating or testing a hypothesis; when capturing a research question; when writing up a result; when working in a repository that holds `questions/`, `hypotheses/`, `theories/` and `research/` records with Q/H/T/R ids; and when accepting a delivered result or assessing a hypothesis. Covers both dispatching the `research_investigation` Orbit job and investigating directly in the current session. Not for ordinary debugging or one-off checks nobody will cite, for reading or web research that produces no recorded result, or for Orbit task and run mechanics, which the orbit and orbit-orchestrate skills cover.
---

# Research loop

A corpus is a Git repository of Markdown records: questions (Q), hypotheses (H), theories (T) and
research results (R) under `questions/`, `hypotheses/`, `theories/` and `research/`, with
`_scripts/schema.json` as the write contract. The `orbit-research` binary writes records; Orbit owns
tasks, runs and delivery. Follow the loop for every experiment worth keeping, whether an Orbit job
investigates or you do.

## Find the corpus

Use the corpus that exists; create one only when there is none. Its root holds
`_scripts/schema.json` and the four record directories: check the current repository, then the
project's instructions, then ask the user. `orbit-research research check --corpus <path>` passes
only on a valid corpus, and every `research` subcommand takes `--corpus`, so work from any directory.

## The loop

Paste the `sh` blocks in order into one shell. They need `jq`, and Git with a `user.name` and
`user.email` because every write commits. `Q001`, `H001` and `R001` are what a fresh corpus
allocates; elsewhere use the ids each command prints. Blocks marked `sh orbit` call Orbit: they need
the plugin enabled for the corpus, which is the Orbit workspace, and start in its checkout.

1. **Open the corpus.** On an existing corpus, set `corpus` to its path and skip `workspace init`.
   Keep drafts in `$scratch`: any uncommitted path in the corpus makes the next write refuse.

```sh
corpus="$PWD/research-corpus"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/research-XXXXXX")   # drafts stay outside the corpus
orbit-research workspace init "$corpus"
orbit-research research check --corpus "$corpus"
orbit-research research list --corpus "$corpus"
```

2. **Capture the question, state the hypothesis, reserve the result**, in that order and before any
   measuring. The hypothesis's claim is its `--body`; make it falsifiable. Reserving allocates the R
   id and commits a stub: nothing else allocates one, and `plan`, `link` and the result writer
   refuse an R that was never reserved. A retry with the same request key replays the first write;
   `capture` derives its key from the text and tags.

```sh
orbit-research research capture --corpus "$corpus" --text "Does the baseline hold under load?" --tag perf
orbit-research research create --corpus "$corpus" --kind H --title "The baseline holds under load" --body "Latency stays within 10 percent of the idle baseline at 2x load." --derived-from Q001 --request-key hypothesis-1
orbit-research research create --corpus "$corpus" --kind R --status planned --title "Load test of the baseline" --derived-from Q001 --derived-from H001 --request-key reserve-1
```

3. **Plan and link.** `plan` drafts an Orbit task for the reserved R and creates nothing. The
   plugin's `link` tool creates that task from the plan's JSON, unchanged, plus `research_id` and
   `request_key`. `link` and `accept` change state, so they need operator capability:
   `ORBIT_OPERATOR=1` on that one command, from a local shell.

```sh
orbit-research --json research plan --corpus "$corpus" --shape investigation --research-id R001 --objective "Measure p99 latency at idle and at 2x load, with a control run" | tee "$scratch/plan.json"
```

```sh orbit
cd "$corpus"
ORBIT_OPERATOR=1 orbit tool run orbit.research.link --input "$(jq --arg id R001 --arg key link-R001 '. + {research_id: $id, request_key: $key}' "$scratch/plan.json")"
```

4. **Investigate.** Run the job with the `task_id` that `link` printed, or do the work yourself
   (see References). The job's agent writes the reserved R in a worktree, `validate` gates it, then
   Orbit commits, merges and moves the task to `review`. Never edit around a `validate` failure.

```sh orbit
cd "$corpus"
orbit run job research_investigation --input task=<task-id>
```

5. **Accept, then assess.** Once the task is `review` or `done`, `accept` re-validates the published
   commit and stores the acceptance on the task. `assess` appends the verdict you state to the
   hypothesis and sets its status; it refuses unless the R was accepted and is unchanged since.

```sh orbit
cd "$corpus"
ORBIT_OPERATOR=1 orbit tool run orbit.research.accept --input '{"task_id":"<task-id>","research_id":"R001"}'
blob=$(orbit-research --json research show --corpus "$corpus" --id H001 | jq -r .git_blob)
revision=$(orbit-research --json research show --corpus "$corpus" --id H001 | jq -r .metadata.revision)
orbit-research research assess --corpus "$corpus" --id H001 --expected-blob "$blob" --research R001 --revision "$revision" --verdict inconclusive --strength anecdote --note "Control run failed; no claim either way"
```

## Writer modes

Every write reports its `mode`; `--mode primary|worktree` makes a mismatch refuse. **Primary** (the
corpus's main checkout) allocates ids and commits: `capture`, `create`, `revise` of Q/H/T,
`revise-question` and `assess`. **Worktree** (a linked worktree) writes only one reserved R's
README and `data/manifest.json`, and never commits.

`revise` and `assess` take `--expected-blob`, the `git_blob` from `show`, so a concurrent change
refuses. Changing a hypothesis's title or body
bumps its revision and reopens it. A committed `done` R is final: never edit it by hand, accepted
or not. To revise it, reserve a new R derived from it.

## Scientific safeguards

- Execution success is not support. A run that finishes, a script that exits 0 or a plot that
  renders is not evidence. Report what the evidence shows, including evidence against the hypothesis.
- A failed control, baseline or sanity check makes the result inconclusive. Say so in Result and
  Limitations and claim neither support nor refutation. The record is still valid and deliverable.
- An investigation never edits an H or T and never appends an assessment. Verdicts are a human step
  after acceptance: `assess` records what its caller states, and acceptance never supplies one.
- Keep conflicting evidence, limitations and lineage. When results disagree, every assessment stays.

## References

Read one when its situation comes up; `orbit-research resource --reference <name>` also prints it.

- [own-session](references/own-session.md): investigating directly instead of dispatching the job.
- [corpus](references/corpus.md): creating a corpus, a corpus not on `main`, or `link` refuses an unprepared one.
- [link](references/link.md): planning a contribution or synthesis, or `link` refuses a scope or key.
- [operator-mcp](references/operator-mcp.md): a mutating tool is refused, or the tools are called over MCP.
- [validate](references/validate.md): the job's `validate` step failed a run.
- [accept-assess](references/accept-assess.md): `accept` or `assess` refuses, or accepted work needs revising.
- [panels](references/panels.md): reading the dashboard to see what needs `accept` or `assess`.
