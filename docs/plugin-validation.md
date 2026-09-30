# Orbit plugin validation

The required local gate is `make test`. It covers the plugin request schemas,
exec input limits, launcher failures and a real Git-backed fixture corpus.
The plugin's eight conformance goldens cover its corpus-independent health
response and deterministic refusals in Orbit's empty conformance workspace.

To exercise an installed plugin with an existing Orbit binary:

```sh
ORBIT_RESEARCH_TEST_ORBIT_BIN=/absolute/path/to/orbit \
  cargo test -p orbit-research-cli --test plugin_v2 --locked -- --ignored
```

This test copies the canonical plugin into a temporary directory, bundles the
current executable and creates its own corpus, HOME and Orbit workspace. It
invokes all eight advertised tools through both Orbit CLI and Orbit MCP:
`version`, `list`, `show`, `check`, `plan`, `link`, `validate` and `accept`.
It checks real task creation and idempotent linking, including an omitted task
description, and malformed `version` fields. `validate` must refuse without
job context; `accept` must refuse a task that has not delivered. CLI calls use
Orbit's explicit operator override and MCP uses its operator session.

Positive delivery validation, positive acceptance, evidence mismatch,
idempotent acceptance and failure cleanup run through the same exec handler
against a Git-backed fixture with an in-memory task host:

```sh
cargo test -p orbit-research-cli --bin orbit-research --locked tests::plugin
```

These deterministic checks do not run an investigation provider. The installed
test changes no existing Orbit workspace or plugin installation. It is ignored
in the ordinary test suite because it needs Orbit's native sandbox; CI runs it
with a checksum-verified Orbit 0.25.0 binary, alongside validation and conformance
testing of a clean Git export.

The manifest requires Orbit 0.25.0 or newer. On macOS, Orbit 0.24.0 denied
metadata access to its callback-session directory, preventing the callback
resolver from recognizing the host's live identity even though the inherited
descriptor remained open. The host fixed this in
[the ancestor metadata grant change](https://github.com/constellation-works/orbit/commit/73beabbc1bfc6091ffdcbea719d234a8a062b825).
The old host passed all eight empty-workspace conformance goldens but failed a
real installed `link` call. Installed callback checks therefore remain a
separate required CI gate. The shared Linux/macOS manifest range keeps the
plugin inactive on Orbit 0.24.0: validation exits successfully with a host
incompatibility warning, installation stages a disabled plugin, and enablement
records grants but reports the plugin as inactive. Tool calls then fail with
`policy_denied` and the required host version before the backend runs.
