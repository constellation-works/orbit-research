# orbit-research-cli

The CLI is the composition boundary for the research application. Keep parsing
in `src/parse.rs`, output policy in `src/output/`, and research/workspace command
surfaces under `src/command/`. Core owns corpus validation and backend authority;
the CLI must not add alternate stores or tool arguments. Stdout is protocol data
and stderr is diagnostics. `src/mcp.rs` owns bounded stdio JSON-RPC transport;
tool definitions and application dispatch remain in Core.

Parser and MCP tests live under `src/tests/`, renderer tests under `src/output/tests/`; golden output fixtures live
under `src/snapshots/`. Composed subprocess tests stay under crate-root `tests/`.
Shared research skills are packaged under Core’s `assets/skills/` and exposed by
CLI. This crate has no packaged tool templates.

Orbit's audit_middleware.rs depends on its runtime and persistent audit store.
Do not copy that dependency into this app. Research mutations retain Git and
Core/Store correlation evidence; execution audit remains owned by Orbit. Add a
CLI audit adapter only against a defined Core contract, not a duplicate engine.

## Terminal interface

Follow [design conventions](../../docs/design/CONVENTIONS.md) and the local
[terminal-interface design](../../docs/design/terminal-interface/2_design.md).
Its [output modes](../../docs/design/terminal-interface/specs/output-modes.md),
[table rendering](../../docs/design/terminal-interface/specs/table-rendering.md)
and [styling](../../docs/design/terminal-interface/specs/color-and-styling.md)
specs govern this crate. Keep the full design folder current with output changes.

Commands return structured values; `output/sink.rs` resolves terminal capabilities
once and `output/render.rs` owns presentation. Default to readable help and human
output, retain explicit JSON/NDJSON, keep diagnostics on stderr and never render
human output into MCP stdout. Every shortened list field needs a documented
[detail command](../../docs/design/terminal-interface/references/detail-commands.md).
