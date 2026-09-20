# orbit-research-cli

The CLI is the composition boundary for the research application. Keep parsing
in `src/parse.rs`, output policy in `src/output/`, and research/workspace command
surfaces under `src/command/`. Core owns corpus validation and backend authority;
the CLI must not add alternate stores or tool arguments. Stdout is protocol data
and stderr is diagnostics.

Unit/renderer/parser tests live under `src/tests/`; golden output fixtures live
under `src/snapshots/`. Composed subprocess tests stay under crate-root `tests/`.
CLI-specific tool templates belong under `assets/tool_templates/`; shared research
skills are packaged by Core under `assets/skills/` and exposed by CLI.

Orbit's audit_middleware.rs depends on its runtime and persistent audit store.
Do not copy that dependency into this app. Research mutations retain Git and
Core/Store correlation evidence; execution audit remains owned by Orbit. Add a
CLI audit adapter only against a defined Core contract, not a duplicate engine.
