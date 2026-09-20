---
title: Implementation slices and acceptance gates
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Implementation slices and acceptance gates implementation contract.
tags: [research-workbench]
---

# Delegation and integration gates

Astra owns architecture, shared contracts, integration and final evidence. Implementers
own only their assigned files, read these contracts first, and surface incompatible
contract changes before editing adjacent layers. Do not use passing fixture tests as
proof of live execution. Do not edit sibling repositories or release packages.

| Slice | Ownership | Required evidence |
|---|---|---|
| Foundation | Astra: crate graph, common contracts, standards | workspace compiles, dependency guard rejects illegal edges, contracts reviewed |
| Persistence hardening | Store worker | parallel reservations, crash/retry, stale edits, symlink and dirty-tree refusal, owner checker compatibility |
| Execution integration | Core worker after contracts freeze | admitted backend, correlation recovery, explicit dispatch/cancel, durable receipt positive/negative tests |
| Transport | CLI/MCP worker | shared operation parity, bounded protocol input, JSON errors and stdout purity, regression compatibility |
| Dashboard | Web worker after API freeze | capture-to-result journey, honest execution/scientific states, accessible/responsive synthetic-corpus browser QA |
| Delivery | Astra + independent Opus | complete repository gate, real Mac managed run, failure recovery, review fixes, PR checks and merge evidence |

Baseline exists but is not delivery: the initial reader/writer/planner and adapter tests
are useful foundations; connected task/run/result surfaces and live QA are mandatory.
Workers may implement independent frozen boundaries concurrently. Shared Core API
changes have one owner; consumers wait or use fixture contracts rather than inventing
parallel APIs. Review is independent of implementation. Opus code review and QA are
required before sign-off. No release/version/tag actions are part of this assignment.

## Foundation exit checklist

Before widening delegation, reconcile owner reservation support with the task contract,
freeze the backend configuration and shared operation DTOs, and check the entire
workspace rather than only new crates. No worker may invent a compatibility entry
to make a smoke test pass. Receipt fixtures and protocol hardening can proceed now
because their boundaries are already isolated. Dashboard/execution integration waits
on those shared contracts.
