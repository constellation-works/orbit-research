---
title: Terminal Interface — Overview
owner: codex
last_updated: 2026-09-20
status: Draft
feature: terminal-interface
doc_role: overview
type: design
summary: "Orbit Research terminal output for humans, scripts and agents, adapted from Orbit conventions."
tags: [terminal-interface, research-workbench]
paths: ["crates/orbit-research-cli/src/**", "crates/orbit-research-cli/tests/**"]
related_features: [research-workbench, user-interface]
related_artifacts: [DANI-10580]
---

# Terminal Interface — Overview

Terminal Interface owns what `orbit-research` writes to stdout and stderr: help,
list layout, detail views, output modes, styling and errors. This adapts the Orbit
repository's terminal-interface design to canonical research records. It shares
vocabulary with the [dashboard](../user-interface/1_overview.md), not rendering
code. Core owns research operations; CLI owns their presentation.

## 1. Motivation

A bare invocation previously returned help inside an escaped JSON error. Ordinary
commands defaulted to JSON even when a person wanted to scan records. [DANI-10580]
addresses both by distinguishing human presentation from explicitly requested
machine output. Scripts must opt into `--format json` or `--format ndjson`.

The same validated payload supplies every rendering. Human tables may shorten a
title or path, but they must identify a way to retrieve it in full. Scientific
status must remain explicit text: a hypothesis is not a successful execution run.

## 2. Core Concepts

- **Payload:** the structured result from Core, retained intact for JSON consumers.
- **Sink:** one resolution of output mode, terminal width and color permission.
- **Renderer:** the only layer that formats payloads into human or machine bytes.
- **Plain list:** stable, headerless TSV for pipes, with full field values.
- **Detail:** a full record selected by ID, including canonical Markdown and identity.

Help/version are human process controls. MCP stdio is a separate JSON-RPC protocol
and never passes through the human renderer.

## 3. At a Glance

| Concern | File | Task |
|---------|------|------|
| Current implementation and limits | [Design](./2_design.md) | [DANI-10580] |
| Mode precedence, errors and streams | [Output modes](./specs/output-modes.md) | [DANI-10580] |
| Width, columns and truncation | [Table rendering](./specs/table-rendering.md) | [DANI-10580] |
| Styling and scientific meaning | [Color and styling](./specs/color-and-styling.md) | [DANI-10580] |
| Full values behind shortened cells | [Detail commands](./references/detail-commands.md) | [DANI-10580] |
| Open questions | [Vision](./3_vision.md) | [DANI-10580] |
| Standing tradeoffs | [Decisions](./4_decisions.md) | [DANI-10580] |

## Task References

- [DANI-10580] — aligns CLI help, human output, machine modes and terminal tests with Orbit's terminal conventions.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
