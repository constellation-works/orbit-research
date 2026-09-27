# orbit-research — agent guide

Loaded as both `AGENTS.md` and `CLAUDE.md`.

## Rules

- Work only on authorized scope. In a managed activity, leave commits and delivery transitions to the pipeline. Neither implementation nor a PR request authorizes merging.
- Don't invent task IDs — get them from `orbit.task.add`. Don't edit task files directly — use `orbit.task.update`.
- Don't add cross-crate dependencies without updating [`ARCHITECTURE.md`](ARCHITECTURE.md).
- Releases require Daniel's explicit authorization. General implementation or merge approval does not authorize version bumps, release tags or package publication.
- Update affected docs in the same PR as the code. Stale docs are a review blocker.

## Code

- Layering and scoping: [`ARCHITECTURE.md`](ARCHITECTURE.md). Reusable patterns: [`docs/design-patterns/`](docs/design-patterns/).
- `[workspace.lints]` denies `dbg_macro` and warns on `print_stdout` and `unwrap_used`. `make clippy` checks all CLI targets with `--no-deps` and denies warnings; it is not a workspace-wide Clippy gate. `make test` is the required validation gate.
- Propagate `orbit_research_common::Error` at fallible boundaries. CLI output belongs in its renderer; libraries return values/errors rather than printing payloads.
- Default to `pub(crate)`; workspace deps via `.workspace = true`; typed `thiserror` variants.
- Unit tests live in a sibling `tests/` dir mirroring source filenames ([`test_layout.md`](docs/design-patterns/test_layout.md)); crate-root `tests/` is integration only.
- Never expose internal task/friction IDs in user-facing output, CLI help (Clap renders `///`), or MCP text.
- Prefer the fewest moving parts: delete dead code and stale docs together, keep compatibility only for an external contract or persisted format, ~800 lines per file is a split signal.
- Report commands and outcomes at handoff — passed, failed, not run — never "tested".

## Orbit Workflow

For any Orbit lifecycle work, invoke the `orbit` skill; its `SKILL.md` routes to the matching reference.

## Backlog hygiene

- Reconcile the owning Orbit task when committing or handing off a change. Record
  the commit, validation, delivery state and remaining work in its execution summary.
- Keep descriptions, acceptance criteria and file selectors aligned with current
  code. Replace obsolete instructions rather than appending contradictory plans.
- Close obsolete or duplicate proposals with evidence and a successor reference
  where applicable. Do not label them implemented when their target was retired.
- Move implemented work out of in-progress. Keep it in review when review is
  deferred; completion needs the applicable validation and delivery evidence.
- Split residual work explicitly: the completed scope and its remaining follow-up
  must each have accurate criteria. Never silently drop an unmet criterion.
- An in-progress task is not evidence of a running agent. Record the run when one
  exists, and mark deliberately parked work with its actual blocker or deferral.
