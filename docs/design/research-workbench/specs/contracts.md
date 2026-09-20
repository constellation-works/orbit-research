---
title: Canonical records and execution contracts
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Canonical records and execution contracts implementation contract.
tags: [research-workbench]
---

# Contracts

## Canonical corpus

The owner schema is authoritative. IDs and paths remain stable when titles change.
Q/H/T/R frontmatter and Markdown remain canonical; project membership is a tag.
Derived-from references preserve lineage, including retired branches. References,
duplicates and cycles must be checked before publication. Scientific assessments are
explicit author actions and never inferred from task or process success.

workspace init creates a generic compatible corpus only in an empty destination.
Existing corpora are validated without overwrite. The app reads record metadata,
Markdown, manifests and artifact digests; experiment scripts/notebooks remain owner
content and are opaque. It neither executes nor semantically indexes that code.

## Writes and recovery

Creation requires a durable request key. Reusing a key with identical content returns
the same reservation; changed content refuses. Allocation, stub commit and recovery
are serialized across cooperating writers through the Git common directory. Workers
cannot allocate from linked worktrees. For an existing owner corpus, reservation
requires its supported writer capability. An app-local lock does not make the owner
scaffolder cooperate: parallel live dispatch stays disabled until that owner contract
is available and verified. Synthetic corpus tests are not proof of that capability. A stale blob or dirty integration tree refuses
an edit before modifying files. Unknown commit or backend submission outcomes require
reconciliation, never blind replay. External/noncooperating writers must not be
silently overwritten. Keep incomplete attempts inspectable.

## Orbit backend

Configuration pins executable, workspace selector, owner host and checkout. Each
release ships an explicit supported version/capability table; unknown versions refuse
before mutation. Binary digest ties checks to the executable actually invoked.
Operator authority is required for dispatch/cancel: a task-authoring MCP session does
not establish it. Preserve caller restrictions; never use unsandboxed agent invocation
as a fallback. Mac QA must exercise the selected managed job including cargo/PTY and
provider requirements, or document a narrower job that does not need those facilities.

Task linking stores correlation before submission. Retry queries the exact correlation
and adopts exactly one match; zero after an unknown outcome or multiple matches is an
explicit unresolved state. Dispatch observes existing task/run correlation before any
new submission. Status reads are fresh Orbit observations with timestamp/source.
Cancellation is explicit and scoped to the linked run.

## Result acceptance

A receipt binds workspace, execution host, task/run, R item, published commit, record
blob and artifact hashes. Accept only successful terminal execution with matching task
correlation, permitted publication ancestry, ordinary committed files, exact hashes,
and complete canonical result sections. A receipt cannot manufacture an assessment.
Persist acceptance evidence before displaying an accepted result. A run that succeeds
without a valid published receipt remains awaiting evidence. Duplicate, stale, forged,
unpublished or cross-item receipts refuse with an actionable reason.

## Common operations

Expose validated reads, Q/H/T/R creation, guarded question edit, contribution/synthesis
planning, task linking, dispatch, status, cancellation and receipt validation through
Core. CLI/MCP/web share semantics and failure behavior. Read and planning operations
never dispatch implicitly. Tool schemas disallow unrecognized fields; corpus scope is
fixed during process composition. Machine stdout contains protocol/output only.
