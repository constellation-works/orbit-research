# Orbit plugin validation

The required local gate is `make test`. It covers the plugin request schemas,
exec input limits, launcher failures and a real Git-backed fixture corpus.
The plugin's eight conformance goldens cover its corpus-independent health
response and deterministic refusals in Orbit's empty conformance workspace.

Before linking work in an existing corpus, prepare its operational journal
outside the plugin sandbox:

```sh
orbit-research workspace prepare-operations /absolute/path/to/corpus
```

New corpora created by `workspace init` prepare this directory automatically
on Linux and macOS. Preparation preserves existing idempotency entries and
the journal's lock while moving them to `_data/orbit-research-operations/`.
The plugin grants writes only to that ignored operational directory and its
`.orbit-research-tmp/` acceptance scratch directory. It does not need to write
Git metadata or canonical research records. An existing unprepared corpus
refuses linking with the preparation command before creating an Orbit task.

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
description, and malformed `version` fields. It first models an unprepared
legacy journal and requires both transports to refuse linking before any task
is created, with the explicit preparation command. Preparation preserves
existing receipt and lock bytes; subsequent linking and retries leave the
scientific commit unchanged and Git status clean. `validate` must refuse without
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
