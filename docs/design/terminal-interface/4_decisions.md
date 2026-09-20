---
title: Terminal Interface — Decisions
owner: codex
last_updated: 2026-09-20
status: Draft
feature: terminal-interface
doc_role: decisions
type: design
summary: "Standing output tradeoffs and the compatibility exception for canonical snapshot JSON."
tags: [terminal-interface, research-workbench]
paths: ["crates/orbit-research-cli/src/**", "crates/orbit-research-cli/tests/**"]
related_features: [research-workbench, user-interface]
related_artifacts: [DANI-10580]
---

# Terminal Interface — Decisions

Entries follow [design conventions](../CONVENTIONS.md): only surprising code or
rules governing future tradeoffs belong here. Module ownership and ordinary
implementation steps remain in [design](./2_design.md).

## Presentation Must Not Alter the Scientific Payload

**Recorded:** 2026-09-20 · [DANI-10580] establishes shared human and machine rendering.

**Context:** A human needs a short table while a caller may need record bodies,
lineage and content identities. Separate command-side human results could drift
from the values used by machine consumers.

**Decision:** Future presentation changes must derive from the same validated
application payload. Layout, shortened cells and styling must never alter that
payload or interpret successful execution as scientific support. Prefer retaining
scientific evidence over saving terminal space when no full-detail path exists.

**Consequences:** Commands remain independent of terminal capability. The real
alternative is separate compact command results, which can be cheaper to collect.
Cost: full payload construction and validation remain necessary even when the
terminal displays only a few fields.

## Preserve Snapshot JSON Instead of Forcing a List Array

**Recorded:** 2026-09-20 · [DANI-10580] changes human defaults while retaining machine results.

**Code anchors:** `crates/orbit-research-cli/src/output/render.rs::render`.

**Context:** Orbit's generic list convention favors JSON arrays, but Research's
existing list response is a snapshot containing records, revision and tags.
Replacing it with an array would discard context and break existing consumers.

**Decision:** Explicit JSON serializes the complete existing snapshot. NDJSON
projects only its records. Help documents choosing JSON for snapshot metadata.

**Consequences:** Existing JSON readers retain their schema, while line-oriented
consumers can opt into NDJSON. The alternative is a uniform array in both modes.
Cost: the two machine modes have different information content; callers needing
revision-level provenance must use JSON.

## Task References

- [DANI-10580] — aligns CLI help, human output, machine modes and terminal tests with Orbit's terminal conventions.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
