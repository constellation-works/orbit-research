# orbit-research-cli

The CLI is the composition boundary for the research application: argument parsing, output, and the MCP and plugin transports. Core owns application policy; see [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Rules

- Keep parsing in `src/parse.rs`, output policy in `src/output/`, and research/workspace command surfaces under `src/command/`.
- Core owns corpus validation and application policy; the CLI must not add alternate stores or tool arguments.
- Transports translate a wire protocol; tool definitions and application dispatch stay in Core. The MCP stdio transport bounds the size of each request it reads.
- Stdout is protocol data and stderr is diagnostics.
- This crate has no packaged tool templates; `.orbit-plugin/plugin.yaml`, `.orbit-plugin/schemas/` and `.orbit-plugin/definitions/` are the Orbit plugin manifest and definitions, not CLI assets.
- Do not copy Orbit's runtime-dependent audit middleware into this app. Research mutations retain Git and Core/Store correlation evidence; execution audit remains owned by Orbit.
- Add a CLI audit adapter only against a defined Core contract, not a duplicate engine.
- `src/panels.rs` owns the dashboard panel sources; their shapes follow Orbit's generic panel renderer, see [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Tests

- Parser, MCP, plugin-transport and panel tests live under `src/tests/`, renderer tests under `src/output/tests/`, and golden output fixtures under `src/snapshots/`.
- Composed subprocess tests and the plugin schema/registry parity check stay under crate-root `tests/`.

## Terminal interface

- Follow [design conventions](../../docs/design/CONVENTIONS.md) and the local [terminal-interface design](../../docs/design/terminal-interface/2_design.md). Its [output modes](../../docs/design/terminal-interface/specs/output-modes.md), [table rendering](../../docs/design/terminal-interface/specs/table-rendering.md) and [styling](../../docs/design/terminal-interface/specs/color-and-styling.md) specs govern this crate.
- Keep the full design folder current with output changes.
- Commands return structured values; the sink (`output/sink.rs`) resolves terminal capabilities once and the renderer (`output/render.rs`) owns presentation.
- Default to readable help and human output, retain explicit JSON/NDJSON, keep diagnostics on stderr and never render human output into MCP stdout.
- Every shortened list field needs a documented [detail command](../../docs/design/terminal-interface/references/detail-commands.md).
