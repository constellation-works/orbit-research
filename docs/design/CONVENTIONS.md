---
title: Design Doc Conventions
owner: grok
last_updated: 2026-09-12
last_validated: 2026-09-12
status: Accepted
type: design
summary: Pointer to Orbit's design-doc layout; orbit-research feature folders live under docs/design/<feature>/.
tags: [design-conventions]
---

# Design Doc Conventions

This repository follows the folder layout, frontmatter, decision admission
rule, and cross-link conventions in the orbit repository's
`docs/design/CONVENTIONS.md`. Do not invent a second documentation system
here.

Feature folders live under `docs/design/<feature>/`:

```
docs/design/<feature>/
├── 1_overview.md
├── 2_design.md
├── 3_vision.md
├── 4_decisions.md
├── specs/
└── references/
```

Decision entries are admitted only when they explain surprising code or
govern a future tradeoff nobody has seen yet. Crate boundaries, slice order,
and the obvious next instance of an existing pattern belong in `2_design.md`.
Every decision carries a `Cost:` line. Task IDs stay plain bracketed text.
