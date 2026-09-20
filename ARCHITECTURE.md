# Architecture

Lower layers do not depend on application or transport layers. Orbit Research is
an independent application; it uses an explicitly selected Orbit CLI, never an
Orbit implementation crate, runtime, scheduler, or internal database.

```text
orbit-research-cli ──┬── orbit-research-web ──┐
                    ├── orbit-research-mcp ──┤
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
  core, loopback HTTP adapter and stdio MCP adapter. `orbit-research` remains the
  installed binary; there is no second workbench binary.
- **Core** owns shared application operations, contribution/synthesis planning,
  backend compatibility and explicit authority checks, task/run coordination,
  and research receipt acceptance. It receives an explicit corpus root. It
  invokes Orbit through argv and structured responses, preserving caller
  restrictions. It never starts another execution engine.
- **Store** owns canonical Markdown/frontmatter reads and guarded writes, Git
  content identities, serialized committed ID reservations, atomic request
  journals, and corpus scaffolding. The operational journal contains request
  correlation and pointers to Orbit, never competing scientific records.
  Unknown outcomes are reconciled; a timeout is not permission to submit twice.
- **Web** owns the loopback HTTP protocol, browser session protections and the
  dashboard. It delegates operations to Core and has no direct store dependency.
- **MCP** owns stdio JSON-RPC transport and delegates tool contracts/operations
  to Core. Its corpus scope is fixed at startup; tool arguments cannot switch
  it to a different filesystem root.

The workspace contains exactly six crates: Common, Store, Core, CLI, Web and MCP.
The former contract/owner/import/index packages are removed. Existing command
compatibility lives in modules: pure legacy schema/digest rules in Common;
owner-file and index/export persistence in Store; import orchestration in Core.
Core exposes the compatibility facade consumed by CLI. These paths never receive
writes from the Markdown application. Scientific authority remains the selected
owner corpus; project membership is a tag, and H/T assessments remain explicit.

## Core module ownership

Core mirrors Orbit's composition conventions while owning its types and utilities.
It does not import Orbit libraries.

```text
orbit-research-core/
├── assets/skills/             # packaged research guidance
└── src/
    ├── application/           # use cases, work planning, receipts and API routing
    ├── bootstrap/             # local/configured assembly and workspace initialization
    ├── runtime/               # process-scoped Application and Research handles
    ├── config/                # operator-selected startup settings
    └── adapter/orbit/         # external CLI invocation, identity and compatibility
```

Transports invoke application use cases. Bootstrap fixes corpus and backend scope
at startup; runtime contains their state. The Orbit adapter owns subprocesses and
external protocol checks, while application operations own research policy and
request reconciliation. Store remains responsible for filesystem and Git writes.
Existing public module aliases preserve callers during this internal reorganization.

Core owns bundled skills; CLI exposes them through its resource command. Future
research routines, auto-tasks, activities and jobs can live under Core assets and
be scaffolded into `.orbit/`. They are not implemented or enabled by this layout.

## Web module ownership

`orbit-research-web/assets/dashboard/` owns the embedded HTML, CSS and JavaScript.
`src/lib.rs` binds the loopback listener and assembles its application/session state.
`src/api/` owns routing, session guards, response headers and Core delegation;
`src/parse.rs` owns HTTP input shapes and extraction; `src/log_format.rs` formats
local server diagnostics. These modules use research types and do not import Orbit
libraries. The legacy static export assets at repository-root `web/` remain separate.

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
Store unit/fixture tests live under `src/tests`; composed integration tests
live in consuming crates. Tests use isolated temporary Git repositories.

`scripts/check-dependency-direction.sh --self-test` enforces the crate graph and
rejects dependencies on Orbit libraries. Update this document with dependency
changes. The repository's `make test` gate remains required, with focused tests
for each new application boundary and independent review/QA before sign-off.
