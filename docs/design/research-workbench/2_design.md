---
title: Research application architecture
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Research application architecture implementation contract.
tags: [research-workbench]
---

# Architecture and ownership

```text
CLI (composition) -> Web -> Core -> Store
                 -> CLI MCP transport -> Core
                 --------> Core
Core -> Common <- Store (workspace leaf)
Core -> external Orbit CLI -> Orbit tasks/runs/artifacts
Store -> owner Markdown/Git + local request log
```

| Crate | Owns | Must not own |
|---|---|---|
| orbit-research-common | passive shared record/reservation types and typed errors | I/O, configuration loading, application decisions, workspace dependencies |
| orbit-research-store | schema validation, Git identity, canonical files, writer lock, reservation intents and request correlations | agent execution, task policy, HTTP/JSON-RPC |
| orbit-research-core | application use cases, work scope, backend admission, execution correlation, receipt acceptance, backend config | browser rendering, CLI formatting, duplicate scientific storage |
| orbit-research-cli | args, configuration composition, central output renderer, stdio MCP transport, process lifetime | direct scientific file mutations |
| orbit-research-web | loopback HTTP, session protection, dashboard | Orbit subprocesses, scientific acceptance rules |

Core is the common operation boundary. Typed request/response contracts belong here;
transport adapters deserialize, call and render. Store contains durable atomicity and
schema checks; Core contains workflow policy. Keep exports narrow, errors typed,
dependencies workspace-managed and tests at the owning boundary. Do not introduce a
crate for each module or import any Orbit implementation crate. Dependency checks are
executable in scripts/check-dependency-direction.sh; ARCHITECTURE.md is the root map.

CLI owns the bounded stdio MCP transport; its startup corpus scope cannot be changed
by tool arguments. BackendConfig and BackendSettings live in Core config, while
Common remains the Store/Core leaf for scientific types and errors.

Core modules: corpus use cases, work planning, backend compatibility/CLI adapter,
operations/correlation, receipt validation, and shared API contracts. Store modules:
corpus reader, guarded writer, request log and workspace scaffold. Preserve these boundaries
if individual modules grow into directories.

There are no separate contract/owner/import/index crates or retained legacy JSON
command paths. Common holds passive research records and errors; Core owns backend configuration; Store
persists Markdown and operational request logs; Core owns application policy.

## Concurrency

Reserve and commit Q/H/T/R identifiers on the integration checkout before dispatch.
Workers receive exact IDs and source revisions, never independently allocate max+1.
For a shared R item, contribution tasks own code/<unit> and artifacts/<unit>. They do
not update the shared README or input manifest. A dependency-gated synthesis task
updates those shared records after contributions land. Independent conclusions use
independent R items with derived_from links. Orbit owns worktree and task coordination.
