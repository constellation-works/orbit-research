# Architecture

Lower layers do not depend on application or transport layers. Orbit Research is
an independent application. It no longer shells out to an Orbit CLI adapter to
drive execution: Orbit owns tasks, crews, runs and delivery natively, and this
app coordinates only through local request correlation. See the plugin
conversion spec (constellation `operations/research/orbit-research-plugin.md`)
for the superseding `plan`/`link`/`validate`/`accept` design, which lands in
later slices.

```text
orbit-research-cli ── orbit-research-core
                              │
                      orbit-research-store
                              │
                      orbit-research-common
```

Core also depends directly on Common for its public operation types. Common is a
workspace leaf; no workspace crate dependency is permitted.

- **Common** owns passive Record, Snapshot and Reservation values and shared typed
  errors. No I/O, application policy, configuration loading or generic utility bucket.
- **CLI** owns argument parsing, process composition and output. It composes the
  core and its stdio MCP adapter. `orbit-research` remains the installed binary;
  there is no second workbench binary.
- **Core** owns shared application operations, contribution/synthesis planning
  and local request correlation. It receives an explicit corpus root; it never
  starts another execution engine and never shells out to Orbit.
- **Store** owns canonical Markdown/frontmatter reads and guarded writes, Git
  content identities, serialized committed ID reservations, atomic request
  logs, and corpus scaffolding. The request log holds writer intents and link
  request keys; it is not a scientific journal or an alternate record store.
- **CLI’s MCP transport** owns stdio JSON-RPC transport and delegates tool contracts/operations
  to the application layer. Its corpus scope is fixed at startup; tool arguments cannot switch
  it to a different filesystem root.
- **CLI’s plugin transport** (`src/plugin.rs`, the `orbit-tool` subcommand) owns the sandboxed
  Orbit plugin `exec` backend protocol: one stdin JSON request, one stdout JSON reply, never a
  nonzero exit for a refused call. It resolves each call's tool verb to a Core `Operation` and
  opens `Application::local` against `context.workspace_root` — the plugin's bound workspace,
  never a request-supplied path, matching the MCP transport's fixed-scope rule above. `version`
  is the one exception: it answers from `orbit_research_core::VERSION` without opening a corpus,
  so it still answers when the bound workspace holds none. `plugin.yaml` (repo root) declares
  `list`, `show`, `check` and `version` as sandboxed, read-only tools with `fs.read: {{workspace}}`
  only; their input schemas in `schemas/*.request.json` are copies of the registry's own derived
  schemas, checked for drift by `crates/orbit-research-cli/tests/plugin_schemas.rs`.

The workspace contains exactly four crates: Common, Store, Core and CLI. The
former contract/owner/import/index packages, their legacy JSON command
surfaces, the standalone dashboard (`orbit-research-web`) and the Orbit CLI
adapter are removed. Scientific authority remains the selected Markdown owner
corpus; project membership is a tag, and H/T assessments remain explicit.

## Core module ownership

Core mirrors Orbit's composition conventions while owning its types and utilities.
It does not import Orbit libraries.

```text
orbit-research-core/
├── assets/                   # skills/ research guidance
└── src/
    ├── application/           # use cases, work planning and request correlation
    ├── bootstrap.rs             # local assembly and workspace initialization
    └── runtime.rs               # process-scoped Application and Research handles
```

Transports invoke application use cases. Bootstrap fixes the corpus scope at
startup; runtime contains its state. Application operations own research policy
and local request correlation. Store remains responsible for filesystem and Git
writes. Common retains scientific values and errors shared by Store and Core; it
never imports Core or reads files.

Packaged files live under each owning crate’s `assets/` directory: Core owns
`skills/`, and Store owns `schema.json`. Do not introduce a parallel `resources/`
directory.

Core owns bundled skills; CLI exposes them through its resource command. Future
research routines, auto-tasks, activities and jobs can live under Core assets and
be scaffolded into `.orbit/`. They are not implemented or enabled by this layout.

CLI `src/mcp.rs` owns stdio protocol framing and session handling, with sibling
tests in `src/tests/mcp.rs`. Core owns the shared operation registry, schemas and
application dispatch; it does not own the MCP transport.

## Store module ownership

`corpus.rs` coordinates validated working-tree and committed reads. `record.rs`
owns canonical Markdown parsing/rendering, filename rules and record scaffolds.
`validation.rs` compiles owner schemas and validates references, numbering and
lineage. `git/` owns Git identity: `git/read.rs` resolves refs, commits, trees
and blobs in-process (loose and packed refs, detached HEAD) with no
subprocess, backing every read and validation path; `git/mod.rs` keeps the
`git`/`git_bytes` process boundary and command-status interpretation, used
only by the primary-mode writer's commit/lock plumbing. A reader cannot reach
a spawning function without leaving `read.rs`.
`writer.rs` owns the common checkout lock and durable create/revision transaction.
Request correlation and initial workspace scaffolding remain in `request_log.rs`
and `workspace.rs`.

Work planning uses `Corpus::committed_snapshot`: schema and records are read from
one pinned commit. Browsing uses `snapshot`, a working-tree view whose revision is
its base HEAD, not a promise that its files are committed. Both hashes for a record
come from the same byte buffer. Writers reuse the compiled owner contract and
refuse a changed schema until the handle is reopened.

Creation and revision persist a complete intent before changing canonical files.
An identical retry resumes that intent, checks exact file bytes and refuses
conflicting edits. Atomic intent replacement and synced temporary file contents
protect the recovery record. Unix also syncs the containing directory after
publication; Windows does not offer a supported directory sync through Rust's
file API, so a sudden power loss can lose a newly published directory entry.
When the intent entry survives, retries reconcile an interrupted operation with
the committed Git state.
These protections do not turn external Git operations into cooperating writers.
See the [write contract](docs/design/research-workbench/specs/contracts.md).

## Operation contracts

`application/operation.rs` is the single registry of typed operation variants,
external names, descriptions, request types and handlers. Request structs in
`application/request.rs` derive both Serde decoding and JSON Schema. The registry
generates tool discovery and dispatch from those same types, so nested work plans,
defaults, required fields and unknown-field rules stay aligned. Derived constraints
are checked before handlers run, using cached compiled validators.

Rust callers use `Application::execute(Operation::ReviseQuestion, arguments)`.
Only external protocol boundaries parse names such as `research.revise_question`.
Adding a tool requires a typed request, handler and registry entry, rather than a
handwritten JSON schema and several string switches.

## Parallel research work

A committed R item exists before work is dispatched. Contributions own disjoint
`code/<unit>/` and `artifacts/<unit>/` paths; their findings do not edit the shared
README or input manifest. A subsequent synthesis task owns those shared files
and depends on completed contributions. Separate investigations with their own
questions and conclusions use separate R items and `derived_from` lineage.
Orbit owns tasks, worktrees, file reservations, run state and delivery history.

## Standards and validation

Keep `mod.rs` focused on module declarations and exports; implementation belongs
in named sibling files (for example CLI `output/render.rs`).

Use workspace dependencies, typed errors and narrow public APIs. Keep
persistence in Store and decisions in Core.
Unit tests follow [the sibling test layout](docs/design-patterns/test_layout.md):
a parent module declares `tests/`, with files mirroring sibling production files.
Tests exercise exposed seams rather than child-module access to private helpers.
End-to-end public crate tests stay under crate-root `tests/`. Git tests use isolated
temporary repositories.

`scripts/check-dependency-direction.sh --self-test` enforces the crate graph and
rejects dependencies on Orbit libraries. Update this document with dependency
changes. The repository's `make test` gate remains required, with focused tests
for each new application boundary and independent review/QA before sign-off.

Rust formatting is enforced by `make fmt-check`.

`Store::request_log` holds writer intents and link request keys. The writer
records reservation intents for crash recovery. Neither is a scientific
journal or an alternate record store; both are required to prevent duplicate writes
and work after uncertain outcomes.
