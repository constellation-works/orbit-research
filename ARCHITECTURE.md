# Architecture

Lower layers do not depend on application or transport layers. Orbit Research is
an independent application; it uses an explicitly selected Orbit CLI, never an
Orbit implementation crate, runtime, scheduler, or internal database.

```text
orbit-research-cli ──┬── orbit-research-web ──┐
                    └───────────────────────┴── orbit-research-core
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
  core, loopback HTTP adapter and Core’s stdio MCP adapter. `orbit-research` remains the
  installed binary; there is no second workbench binary.
- **Core** owns shared application operations, contribution/synthesis planning,
  backend compatibility and explicit authority checks, task/run coordination,
  research receipt acceptance, backend configuration and the stdio MCP adapter.
  It receives an explicit corpus root and invokes Orbit through argv and structured
  responses, preserving caller
  restrictions. It never starts another execution engine.
- **Store** owns canonical Markdown/frontmatter reads and guarded writes, Git
  content identities, serialized committed ID reservations, atomic request
  logs, and corpus scaffolding. The request log contains request
  correlation and pointers to Orbit, never competing scientific records.
  Unknown outcomes are reconciled; a timeout is not permission to submit twice.
- **Web** owns the loopback HTTP protocol, browser session protections and the
  dashboard. It delegates operations to Core and has no direct store dependency.
- **Core’s MCP adapter** owns stdio JSON-RPC transport and delegates tool contracts/operations
  to the application layer. Its corpus scope is fixed at startup; tool arguments cannot switch
  it to a different filesystem root.

The workspace contains exactly five crates: Common, Store, Core, CLI and Web.
The former contract/owner/import/index packages and their legacy JSON command
surfaces are removed. Scientific authority remains the selected Markdown owner
corpus; project membership is a tag, and H/T assessments remain explicit.

## Core module ownership

Core mirrors Orbit's composition conventions while owning its types and utilities.
It does not import Orbit libraries.

```text
orbit-research-core/
├── assets/skills/             # packaged research guidance
└── src/
    ├── application/           # use cases, work planning, receipts and API routing
    ├── bootstrap/             # local/configured assembly and workspace initialization
    ├── config/                # operator-selected backend settings
    ├── runtime/               # process-scoped Application and Research handles
    └── adapter/
        ├── mcp.rs             # bounded stdio transport
        ├── tests/mcp.rs       # transport tests through its public surface
        └── orbit/            # external CLI invocation, identity and compatibility
```

Transports invoke application use cases. Bootstrap fixes corpus and backend scope
at startup; runtime contains their state. The Orbit adapter owns subprocesses and
external protocol checks, while application operations own research policy and
request reconciliation. Store remains responsible for filesystem and Git writes.
`BackendSettings` and `BackendConfig` live in Core’s `config/`: only application
composition and backend execution need them. Bootstrap loads them and the Orbit
adapter validates them. Common retains scientific values and errors shared by Store
and Core; it never imports Core or reads files.

Core owns bundled skills; CLI exposes them through its resource command. Future
research routines, auto-tasks, activities and jobs can live under Core assets and
be scaffolded into `.orbit/`. They are not implemented or enabled by this layout.

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

## Web module ownership

`orbit-research-web/assets/dashboard/` owns the embedded HTML, CSS and JavaScript.
`src/lib.rs` binds the loopback listener and assembles its application/session state.
`src/api/` uses a flat method/path match and a shared request wrapper for session
guards, bounded JSON decoding and response headers; handlers delegate to Core;
`src/parse.rs` owns HTTP input shapes and extraction; `src/log_format.rs` formats
local server diagnostics. These modules use research types and do not import Orbit
libraries. The legacy static export implementation has been removed.

## Parallel research work

A committed R item exists before work is dispatched. Contributions own disjoint
`code/<unit>/` and `artifacts/<unit>/` paths; their findings do not edit the shared
README or input manifest. A subsequent synthesis task owns those shared files
and depends on completed contributions. Separate investigations with their own
questions and conclusions use separate R items and `derived_from` lineage.
Orbit owns tasks, worktrees, file reservations, run state and delivery history.

## Standards and validation

Use workspace dependencies, typed errors and narrow public APIs. Keep
persistence in Store, decisions in Core and protocol concerns in adapters.
Unit tests follow [the sibling test layout](docs/design-patterns/test_layout.md):
a parent module declares `tests/`, with files mirroring sibling production files.
Tests exercise exposed seams rather than child-module access to private helpers.
End-to-end public crate tests stay under crate-root `tests/`. Git tests use isolated
temporary repositories.

`scripts/check-dependency-direction.sh --self-test` enforces the crate graph and
rejects dependencies on Orbit libraries. Update this document with dependency
changes. The repository's `make test` gate remains required, with focused tests
for each new application boundary and independent review/QA before sign-off.

Rust formatting is enforced by `make fmt-check`. Dashboard HTML/CSS/JavaScript use
pinned Prettier 3.6.2 through `make fmt-dashboard` and `make fmt-check-dashboard`.
Node/npm are needed only for dashboard formatting, not for the Rust runtime.

`Store::request_log` holds only dispatch idempotency and Orbit task/run pointers.
The writer records reservation intents for crash recovery. Neither is a scientific
journal or an alternate record store; both are required to prevent duplicate writes
and work after uncertain outcomes.
