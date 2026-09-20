---
title: Research dashboard design
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Research dashboard design implementation contract.
tags: [research-workbench]
---

# Research dashboard

A calm overview of research and its execution. Reuse Orbit's typography, restrained
dark palette, compact controls and layout conventions, while keeping the research
application and navigation distinct. See [layout and states](2_design.md).

Navigation: Overview, Questions, Research, Hypotheses, Theories. Tags filter across
views, including project tags. Overview emphasizes questions needing attention,
investigations in progress and results awaiting evidence; avoid an undifferentiated
activity feed. Record detail includes source path/revision, lineage, scientific content,
linked work and evidence. Retired branches remain discoverable.

Creation and edits use Core operations. The dashboard never writes Markdown itself.
