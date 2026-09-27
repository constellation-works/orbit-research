---
title: Research workbench
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Research workbench implementation contract.
tags: [research-workbench]
---

# Research workbench

> **Partially superseded (2026-09-27):** the standalone dashboard, the Orbit CLI
> adapter (`backend`/`work_status`/`promote`/`dispatch`/`cancel`) and the agent-
> supplied receipt model described below are removed. Orbit owns execution
> through its own native commands, and `link`/`validate`/`accept` return as
> plugin tools in later slices. See the constellation
> `operations/research/orbit-research-plugin.md` spec. Kept for history.

A separate local application for managing scientific questions and investigations.
Orbit owns execution; the corpus owns scientific knowledge. A successful run never
establishes scientific support.

The first delivery is one complete journey: capture a question, reserve an investigation,
link and execute scoped work through Orbit, and accept its published result using a
validated receipt. Q/H/T/R creation is supported; hypotheses, theories and explicit
assessments are read views beyond creation. Projects are tags. Preserve lineage and
retired branches in the index. Scripts and notebooks are opaque artifacts, not indexed code.

See [architecture](2_design.md), [operation contracts](specs/contracts.md),
[delivery gates](specs/delivery.md), [dashboard](../user-interface/1_overview.md),
and [terminal](../terminal-interface/1_overview.md).

This is a target contract, not evidence of implementation completion. Delivery status
belongs in the assignment and verification artifacts. Legacy registry/import/static
export interfaces remain supported without dual-writing Markdown records.
