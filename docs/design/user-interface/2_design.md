---
title: Dashboard layout and state contract
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Dashboard layout and state contract implementation contract.
tags: [research-workbench]
---

# Layout and state

Use a stable sidebar, concise page header, tag/search controls and a readable record
list/detail layout. Responsive layouts collapse navigation and stack detail; do not
clip controls or force whole-page horizontal scrolling. IDs, revisions and metrics
use monospace; prose uses a readable sans-serif at approximately 14px/1.5.

Tokens follow Orbit: base #000/#0a0a0a, border #2a2a2e, subtle separator #17171a,
foreground #dcdcdc, muted #71717a, accent #6e9fff. Small 2/4/6px radii, fine borders,
minimal animation. Execution statuses may reuse Orbit semantic colors, but always
include text. Scientific support/refutation/inconclusive labels remain separate from
execution success/failure and are never colored as if process success proves truth.

Every view has explicit loading, empty, failure and stale-observation states. Preserve
user input after errors. Creation exposes returned ID/commit. Stale edit conflicts
show the current source and require refresh/reconciliation. Dispatch is an explicit
action showing corpus, task, crew and backend target; never a side effect of capture.
Unknown submission state offers reconciliation, not an unqualified retry button.

Record details distinguish proposed work, running work, successful execution awaiting
receipt, accepted published results and explicit scientific assessments. Show artifact
provenance and limitations; scripts/notebooks are opaque links. Render untrusted
Markdown without executable HTML. Keyboard navigation, visible focus, labeled forms,
dialog focus management and status text are required acceptance conditions.

HTTP binds loopback only, validates Host/Origin and mutation session tokens, bounds
input and reports per-request failures without terminating the server. No arbitrary
filesystem read routes, arbitrary root switching or shell-command fields.
