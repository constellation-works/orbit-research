---
title: Terminal Interface — Design
owner: codex
last_updated: 2026-09-20
status: Draft
feature: terminal-interface
doc_role: design
type: design
summary: "Current CLI sink, record rendering, help and errors, with explicit implementation limitations."
tags: [terminal-interface, research-workbench]
paths: ["crates/orbit-research-cli/src/**", "crates/orbit-research-cli/tests/**"]
related_features: [research-workbench, user-interface]
related_artifacts: [DANI-10580]
---

# Terminal Interface — Design

This describes the current Orbit Research CLI implementation for [DANI-10580].
The [specs](./specs/output-modes.md) prescribe its contracts; the
[decisions](./4_decisions.md) explain durable tradeoffs. Orbit's terminal design is
the precedent, but its task commands, historical migrations and output libraries
are not imported into this application.

## 1. Output Module and Command Boundary

`crates/orbit-research-cli/src/output/mod.rs` declares and exports modules only.
`sink.rs` resolves process capabilities; `render.rs` projects application JSON
values and errors; `table.rs` lays out research lists. Commands return structured
values without querying the terminal. `main.rs` passes one sink to the renderer
and handles write errors. No Orbit library is a dependency.

Core's typed operation registry remains shared by CLI, Web and MCP. CLI
`command/research.rs::show` selects a record from Core's validated list result;
it does not read Markdown independently or introduce another store.

## 2. Help and Parsing

`parse.rs` defines Clap descriptions, options and examples. A bare invocation
prints long root help to stdout and exits 0. Explicit help and root version also
exit 0 with readable text, even with `--format json`. A missing required argument
or unknown command is a usage error on stderr with exit 2. `resource --version 1`
selects the packaged resource version and is distinct from root `--version`.

## 3. Sink Resolution

`OutputSink::from_process` resolves stdout TTY state once. Explicit `--format`
outranks `ORBIT_RESEARCH_FORMAT`, which outranks `auto`. Auto selects a table on a
terminal and plain output otherwise. Width is positive `COLUMNS`, then Unix
`TIOCGWINSZ`, then 0 (unbounded). Nonterminal output is always unbounded.

Color is allowed only for terminal tables, with nonempty `NO_COLOR` and
`TERM=dumb` disabling it. Machine modes and pipes never receive styling.
`CLICOLOR_FORCE` cannot override this invariant. No process-global sink is used.

## 4. Research Lists

`research list --corpus PATH` renders canonical records as ID, KIND, STATUS,
TITLE, TAGS and PATH. Auto terminal mode suppresses uniform KIND, STATUS, TAGS
and PATH columns; ID and TITLE remain. Explicit table mode keeps all columns
before width fitting. Plain output always contains all six fields without a
header. It escapes embedded controls so each record remains one physical line.

Tables use computed Unicode display widths and two-space gutters, no borders.
TITLE, TAGS and PATH shrink to a floor of eight columns, then drop from the right
with a notice on stderr. IDs, kinds and statuses never shorten. Prose truncates
at the tail; paths in the middle. Missing human table values use `-`.

`research show --corpus PATH --id Q001` prints the full record, including its
canonical body, metadata, path and content identities. See
[detail commands](./references/detail-commands.md) for every shortened column.

## 5. Detail, Resources and Machine Output

Other structured values render as untruncated dotted field labels and values.
Arrays repeat the field label; empty arrays and nulls display `-`. Packaged
`resource` instructions render as Markdown text in human modes. Control characters
are escaped; detail bodies retain newlines and tabs for readability.

JSON preserves the existing complete result shape, including the list snapshot's
revision and tags. It is pretty on a terminal and compact in a pipe. NDJSON emits
one record per line for a list, flushing after each record; singleton results
produce one line. It omits list envelope metadata by design.

## 6. Streams and Errors

Payloads and explicit help/version go to stdout; errors and dropped-column or
empty-list diagnostics go to stderr. Human errors are readable text. Explicit
JSON/NDJSON errors retain the existing nested `error` envelope and optional
validation problems. Usage failures exit 2, operation failures exit 1 and success
exits 0. `finish_output` maps stdout `BrokenPipe` to quiet success.

MCP framing in `src/mcp.rs` bypasses the output renderer entirely. Its stdout
remains JSON-RPC. There are no progress bars or spinners in the current CLI.

## 7. Validation

Sibling unit tests in `src/output/tests/` cover sink modes, Unicode layout,
truncation, column dropping, stable pipe fields and machine serialization.
`tests/terminal_ux.rs` exercises the compiled binary's help/version, resource
version, errors, environment precedence, list and full record detail using an
isolated disposable corpus. Other subprocess tests explicitly request JSON when
they parse JSON. Tests follow [test layout](../../design-patterns/test_layout.md).

## 8. Concerns & Honest Limitations

- Changing the default from JSON to auto requires existing scripts to add
  `--format json` or set `ORBIT_RESEARCH_FORMAT=json`. The JSON schema is unchanged.
- Non-list human results use generic dotted fields rather than bespoke layouts;
  arrays of objects are verbose. They are not truncated.
- Width fitting measures display columns, but truncation walks characters rather
  than extended grapheme clusters; complex emoji or combining sequences may split.
- A terminal narrower than the fixed columns can still overflow. Identifiers and
  scientific status take precedence over fitting an impossible width.
- Machine errors still classify operational failures as `invalid-input`; this
  preserves the existing shape but is not a complete error taxonomy.
- NDJSON flushes per record after Core has collected and validated the corpus;
  it is not incremental filesystem streaming.
- Help is Clap-generated text. Sink policy governs result rendering, not help
  layout. No CLI-specific semantic color mapping or contrast audit exists yet.
- `research show` reads a full validated snapshot; a single damaged record can
  prevent detail access. It intentionally shares Core's validation contract.

## Task References

- [DANI-10580] — aligns CLI help, human output, machine modes and terminal tests with Orbit's terminal conventions.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
