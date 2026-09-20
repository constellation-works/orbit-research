---
type: design
summary: "Mode precedence, machine compatibility, streams, errors and migration."
---

# Spec: Output Modes

Every ordinary command returns a structured payload. One sink resolves its
presentation; explicit help/version and MCP protocol traffic are separate.

## Why This Exists

Readable help and predictable pipes cannot depend on each command making its own
TTY or formatting decision. See [payload decision](../4_decisions.md#presentation-must-not-alter-the-scientific-payload).

## 1. Resolution

Precedence: explicit `--format auto|table|json|ndjson`, then
`ORBIT_RESEARCH_FORMAT`, then `auto`. Auto resolves to table on stdout TTY and
plain otherwise. Width and color are resolved only in `output/sink.rs`.

| Mode | Record list | Other result |
|------|-------------|--------------|
| auto, terminal | Compact table | Full labeled detail or resource text |
| auto, pipe | Headerless full TSV | Full labeled detail or resource text |
| table | Table with stable columns before width fitting | Full labeled detail |
| json | Complete canonical snapshot | Complete JSON value |
| ndjson | One record per line | One JSON value per line |

Explicit table output in a pipe is header-bearing, untruncated and uncolored.
JSON is pretty on a terminal and compact otherwise. NDJSON flushes every record.
Neither machine mode truncates or injects display-only fields.

## 2. Payload Compatibility

Preserve canonical field types, names, body text and identifiers. Lists retain
snapshot metadata in JSON; NDJSON carries records only. Do not add display nulls
or formatted timestamps to change the application's schema. Canonical absent
fields remain as provided by Core. Machine consumers must select a machine mode.

## 3. Streams and Exit Codes

- stdout: successful payload or requested help/version only.
- stderr: errors, empty-list notices and column-drop warnings.
- 0: success, requested help/version, bare root help, or closed stdout pipe.
- 1: command failure. 2: invalid command-line usage.
- JSON/NDJSON errors: one JSON object on stderr with nested `error.code`,
  `error.message`, and optional `error.problems`; stdout remains empty.
- Human errors: readable text on stderr, never escaped JSON containing usage.
- Progress, if later introduced, is terminal-only on stderr and forbidden in
  machine modes. None is implemented today.

## 4. Protocol Exceptions

`mcp` stdout is exclusively JSON-RPC; `--format` cannot change it. Help and root
version always remain readable text, even under a machine format preference.
`resource --version` selects a resource revision rather than process version.

## 5. Migration

The previous default was JSON. Scripts must add `--format json` or set
`ORBIT_RESEARCH_FORMAT=json`; explicit JSON retains its payload schema. Bare
invocation is now successful help. Do not copy Orbit's legacy `--json` flags or
its task-specific error envelope into Research.
