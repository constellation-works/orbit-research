# The `validate` delivery gate

The `research_investigation` job runs the plugin's `validate` tool on the run worktree
after the agent finishes and before the job commits. Any finding fails the run;
the worktree keeps the agent's files for inspection. `accept` runs the same checks
again on the published commit.

In a run worktree the gate compares the checkout with its base: the commit where the
run's branch left the primary checkout's history. The schema and every other record are
read from that base, so a change the run commits itself is still the run's change.

`validate` checks a run's delivery: called outside a run it answers
`run_context_required`, so it cannot check a record you wrote in your own session.

| Finding | Cause | Fix |
|---|---|---|
| `research_unbound` | No R was written with the worktree writer, or the id is not an R at the base | Write the reserved R with `research revise --mode worktree` |
| `record_invalid` | The README does not parse or fails the owner schema | Fix the frontmatter the message names |
| `section_missing` | `Question`, `Method`, `Result`, `Limitations`, `Next` are not all present in that order | Restore the five `##` sections |
| `section_placeholder` | A section is empty or still holds the stub's text | Write the section |
| `orbit_task_mismatch`, `orbit_run_mismatch` | `orbit.task` or `orbit.run` does not name this run | Pass the run's own task and run ids to `--orbit-task` and `--orbit-run` |
| `manifest_invalid` | `data/manifest.json` is absent, not JSON or fails the schema | Rewrite it with `--manifest-file` |
| `artifact_digest_mismatch` | An input under `data/` does not match its manifest `sha256` or `size` | Correct the manifest entry or restore the bytes |
| `dangling_lineage` | A `derived_from` or `tests` target does not exist | Name an existing record |
| `id_allocated` | The worktree added a record that is not at the base | Remove it; reserve ids on the primary checkout |
| `other_record_changed` | A record other than the R changed or disappeared | Revert it |
| `contract_changed` | `_scripts/schema.json` differs from the base | Restore it. A schema change is its own commit on the primary checkout, outside a run |
| `corpus_invalid` | The corpus fails `research check` | Run `research check` and fix what it lists |

Fix the cause and rerun the job. Do not edit around the gate.

Inputs whose bytes are not in the checkout are reported as `unverified_inputs`, not as
findings: data bytes are never committed, so only the manifest travels.
