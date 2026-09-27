# orbit-research-cli

The CLI is the composition boundary for the research application. Keep parsing
in `src/parse.rs`, output policy in `src/output/`, and research/workspace command
surfaces under `src/command/`. Core owns corpus validation and application policy;
the CLI must not add alternate stores or tool arguments. Stdout is protocol data
and stderr is diagnostics. `src/mcp.rs` owns bounded stdio JSON-RPC transport;
`src/plugin.rs` owns the sandboxed Orbit plugin `exec` backend transport (the
`orbit-tool` subcommand: one stdin JSON request, one stdout JSON reply). Both
transports translate a wire protocol; tool definitions and application dispatch
remain in Core.

Parser, MCP and plugin-transport tests live under `src/tests/`, renderer tests
under `src/output/tests/`; golden output fixtures live under `src/snapshots/`.
Composed subprocess tests and the plugin schema/registry parity check stay
under crate-root `tests/`. Shared research skills are packaged under Core’s
`assets/skills/` and exposed by CLI. This crate has no packaged tool templates;
`.orbit-plugin/plugin.yaml`, `.orbit-plugin/schemas/*.request.json` and `.orbit-plugin/definitions/{jobs,activities}/`
are the Orbit plugin manifest and definitions, not CLI assets.
`tests/research_job.rs` drives the plugin job step by step against this binary.

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
