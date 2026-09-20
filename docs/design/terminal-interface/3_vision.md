---
title: Terminal Interface — Vision
owner: codex
last_updated: 2026-09-20
status: Draft
feature: terminal-interface
doc_role: vision
type: design
summary: "Open questions for research detail, lineage, scientific status and future terminal enforcement."
tags: [terminal-interface, research-workbench]
paths: ["crates/orbit-research-cli/src/**", "crates/orbit-research-cli/tests/**"]
related_features: [research-workbench, user-interface]
related_artifacts: [DANI-10580]
---

# Terminal Interface — Vision

This document collects unresolved questions, not implementation promises. The
current behavior is in [design](./2_design.md). [DANI-10580] establishes the basic
human/machine split without expanding the product's execution scope.

## 1. Open Questions

1. **Research detail:** should Markdown bodies appear before identity fields, or
   should a separate raw-body command serve reading and editing workflows?
2. **Lineage:** can question and hypothesis ancestry be legible in a composable
   list without hiding abandoned branches or requiring an interactive tree?
3. **Scientific vocabulary:** which statuses deserve emphasis without conflating
   published results, evidential support and successful task execution?
4. **Large corpora:** when does full-snapshot validation need bounded queries,
   pagination or incremental output with explicit revision consistency?
5. **Enforcement:** should CI reject terminal probes or result printing outside
   the output boundary, beyond behavioral tests and code review?

## 2. Prior Work

### Orbit House Style

The Orbit repository's `docs/design/terminal-interface/` supplies the sink,
borderless table, stderr and detail-path conventions adapted here. Its historical
migration tasks and execution-status palette belong to Orbit, not this repository.

### Existing Research Surfaces

The [research workbench](../research-workbench/1_overview.md) and
[dashboard](../user-interface/1_overview.md) define the scientific record and
interaction boundaries. CLI presentation must not create a second scientific
schema or infer assessments from run outcomes.

### Structured Tool Access

Core's typed tool registry supplies MCP and CLI operations. It provides an
existing machine interface; a new agent-only text format would need evidence
that JSON/NDJSON and MCP are insufficient.

## 3. What May Be Distinctive

The workbench separates scientific conclusions from execution receipts. Terminal
output must keep that separation visible even when the same operator reads a
research item and its linked Orbit task. A compact view is useful only if it
preserves that distinction and provides full evidence on demand.

## 4. References

- Orbit-internal precedent: Orbit repository, `docs/design/terminal-interface/`.
- Local contracts: [output modes](./specs/output-modes.md),
  [table rendering](./specs/table-rendering.md),
  [color and styling](./specs/color-and-styling.md).
- External conventions: Unix stdout/stderr composition and `NO_COLOR`; no new
  external product research was performed for this adaptation.

## Task References

- [DANI-10580] — aligns CLI help, human output, machine modes and terminal tests with Orbit's terminal conventions.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
