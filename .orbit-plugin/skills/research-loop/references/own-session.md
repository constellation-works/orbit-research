# Investigate in your own session

When you run the experiment yourself instead of dispatching the job, do steps 1 and 2, then write
the reserved R from a linked worktree of the corpus: the primary checkout refuses to write an R.
`--test` names the hypothesis the result tests. Step 3 is optional and only adds a task that tracks
the work; name it on `revise` with `--orbit-task <task-id>`.

```sh
blob=$(orbit-research --json research show --corpus "$corpus" --id R001 | jq -r .git_blob)
git -C "$corpus" worktree add -b investigate-R001 "$scratch/R001"
# Run the experiment here, then draft "$scratch/body.md" and "$scratch/manifest.json".
orbit-research research revise --corpus "$scratch/R001" --mode worktree --id R001 --expected-blob "$blob" --status done --test H001 --body-file "$scratch/body.md" --manifest-file "$scratch/manifest.json"
orbit-research research check --corpus "$scratch/R001"
git -C "$scratch/R001" add research
git -C "$scratch/R001" commit -m "Record the R001 result"
git -C "$corpus" merge --ff-only investigate-R001
git -C "$corpus" worktree remove "$scratch/R001"
```

- The body is exactly `## Question`, `## Method`, `## Result`, `## Limitations` and `## Next`, in
  that order, each written. Outside a run nothing checks this: `check` passes stub text.
- The manifest lists every input (`name`, `source`, `sha256`, `size`, `fetch`), or is
  `{"inputs": []}`. In the worktree's R directory, code goes under `code/`, outputs under
  `artifacts/` and input bytes under `data/`, which stays ignored.
- Finish the record before you commit: the writer refuses a committed `done` R.
- Stop there. `accept` refuses a task with no recorded run and `assess` refuses a result with no
  acceptance, so this result cannot carry a verdict today. State the finding in Result, leave the
  hypothesis alone and tell the user it is unassessed. Never invent a run id to get past this. For
  an assessable result, reserve a new R and run steps 3 to 5.
