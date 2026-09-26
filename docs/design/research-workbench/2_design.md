---
title: Research Application — Design
owner: codex
last_updated: 2026-09-20
status: Draft
feature: research-workbench
doc_role: design
type: design
summary: Research application architecture implementation contract.
tags: [research-workbench]
paths: ["crates/orbit-research-core/src/**", "crates/orbit-research-store/src/**"]
related_features: [terminal-interface, user-interface]
related_artifacts: [DANI-10590]
---

# Research Application — Design

This document describes application and persistence ownership. [DANI-10590]
separates Store mechanics and makes snapshot and recovery semantics explicit.

## 1. Crate Boundaries

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
corpus reader, record codec/scaffolds, validation, Git access, guarded writer,
request log and workspace scaffold. Module roots contain declarations and exports.

There are no separate contract/owner/import/index crates or retained legacy JSON
command paths. Common holds passive research records and errors; Core owns backend configuration; Store
persists Markdown and operational request logs; Core owns application policy.

## 2. Concurrency

Reserve and commit Q/H/T/R identifiers on the integration checkout before dispatch.
Workers receive exact IDs and source revisions, never independently allocate max+1.
For a shared R item, contribution tasks own code/<unit> and artifacts/<unit>. They do
not update the shared README or input manifest. A dependency-gated synthesis task
updates those shared records after contributions land. Independent conclusions use
independent R items with derived_from links. Orbit owns worktree and task coordination.

## 3. Store Read Boundaries

`corpus.rs` coordinates reads. `record.rs` owns frontmatter/body encoding and
canonical names; `validation.rs` owns the compiled owner contract and whole-corpus
checks. Reference targets are validated across all records before lineage traversal,
which shares its completed-node set rather than revisiting every ancestry chain.
`git.rs` distinguishes a negative ancestry test from a failed Git command and keeps
binary reads separate from trimmed command output.

`snapshot()` is a validated working-tree view for browsing and editing. Its
`revision` is the base HEAD. `committed_snapshot()` resolves HEAD once, reads both
schema and records from that immutable commit and supplies every work plan and plan
validation. Uncommitted findings do not silently enter a dispatch plan. Record
SHA-256 and Git blob identity are computed from the same bytes; no second path read
can assign a different file version's hash to a record.

The opened working contract is compiled once and reused for reads and writes.
Committed reads reuse it when the committed schema is identical; otherwise they
compile the schema from the selected commit. A changed working schema requires
reopening the writer handle.

## 4. Store Write Transaction

`writer.rs` retains one shared integration-checkout lock and a private `Writer`
for both reservation and question revision. Public operations prepare a canonical
record; the writer persists the intent, applies owned files, validates, stages,
commits and persists the receipt. New requests require a clean primary checkout.

Creation retains the caller's request key. Revision derives its retry identity
from record ID, expected blob and the requested title/body/tags. The persisted
revision intent includes original and intended bytes. A retry may encounter the
original file or the exact intended replacement; any different bytes refuse.
Snapshot record paths and persisted intent paths use Git's forward-slash form on
every platform. Loading an older intent also normalizes native separators before
comparing its owned paths with Git's staged paths.

Intents are written through synced temporary files and atomically published, then
the parent directory is synced. Canonical files use the same atomic publication
mechanism. Existing revision file permissions are retained. A failed commit leaves
inspectable intent and file state; it does not roll back potentially external edits.

Recovery verifies the request trailer and parent commit when the receipt was not
saved. It checks every owned file with exact bytes, including an R item's manifest,
and rejects unrelated changed paths. Completed retries return the saved commit
rather than allocating again. Older creation intents without the new `files` field
are read using their existing path/text plus the canonical R manifest.

## 5. Concerns & Honest Limitations

- The lock coordinates this writer only. Owner scaffolders and external Git or
  filesystem writers must cooperate; validation is not a substitute for that
  authority contract. Path checks are not a hostile-filesystem sandbox.
- Recovery after commit but before receipt currently recognizes the current HEAD
  only. If unrelated commits have advanced it, the operation refuses for manual
  reconciliation rather than guessing which commit completed the request.
- Working-tree reads can span concurrent filesystem edits. They are not atomic
  snapshots; only committed reads promise one immutable revision.
- Each record still requires Git subprocess work. No persistent index or new
  storage service is introduced. Optimize process count only with measured need.
- Directory synchronization is exercised on the supported Mac/Unix execution
  environment; this change does not certify Windows filesystem behavior.
- No automatic rollback or intent deletion is provided. Recovery evidence stays
  inspectable. Review remains deferred at Daniel's request.

## Task References

- [DANI-10590] — fixes lineage failure handling, separates Store modules, pins work
  plans to committed snapshots and unifies creation/revision recovery.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
