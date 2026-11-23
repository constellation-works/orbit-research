# orbit-research-cli

The CLI is the composition boundary for the research application: argument parsing, output, and the MCP and Orbit plugin transports. Core owns corpus validation, application policy, tool definitions and dispatch.

## Rules

- Do not add alternate stores or tool arguments in the CLI. Transports only translate a wire protocol; see the transport entries in [`ARCHITECTURE.md`](../../ARCHITECTURE.md).
- Do not copy Orbit's audit middleware dependency on its runtime and persistent audit store into this app.
- Research mutations keep Git and Core/Store correlation evidence; execution audit stays with Orbit.
- Add a CLI audit adapter only against a defined Core contract, never as a duplicate engine.
- The plugin manifest, request schemas, definitions and research skill live in `.orbit-plugin/` at the repository root. This crate has no packaged tool templates.
- Dashboard panel shapes are fixed by Orbit's generic panel renderer; read the dashboard panels entry in [`ARCHITECTURE.md`](../../ARCHITECTURE.md) before changing them.

## Terminal interface

- Follow [design conventions](../../docs/design/CONVENTIONS.md) and the [terminal-interface design](../../docs/design/terminal-interface/2_design.md); its [output modes](../../docs/design/terminal-interface/specs/output-modes.md), [table rendering](../../docs/design/terminal-interface/specs/table-rendering.md) and [styling](../../docs/design/terminal-interface/specs/color-and-styling.md) specs govern this crate.
- Keep the full design folder current with output changes.
- Commands return structured values; the output renderer owns presentation and resolves terminal capabilities once.
- Default to readable help and human output, retain explicit JSON/NDJSON, keep diagnostics on stderr (stdout is protocol data) and never render human output into MCP stdout.
- Every shortened list field needs a documented [detail command](../../docs/design/terminal-interface/references/detail-commands.md).
