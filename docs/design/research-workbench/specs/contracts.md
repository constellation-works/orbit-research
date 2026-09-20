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

## Why This Exists

The research application must preserve scientific provenance and prevent duplicate
writes while coordinating work through an external Orbit backend.

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

Working-tree reads expose HEAD as their base revision, not as the identity of
uncommitted content. Work plans and their admission checks must use a committed
snapshot: owner schema and record bytes from one resolved commit. Hashes for a
record must describe the exact bytes read. Commit findings before including them
in a work plan.

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

Both creation and revision persist an atomic intent before replacing canonical
files. Revision retry identity includes the expected blob and complete requested
content. The same retry may resume original or intended bytes; different external
edits refuse without overwrite. Intent publication syncs the complete file and its
containing directory. Recovery checks exact committed bytes for every owned file,
including manifests; whitespace differences are differences. A failed Git hook or
commit leaves recoverable state and never triggers blind rollback. If HEAD moved
past an incomplete operation's recognizable commit, require manual reconciliation.

The writer refuses a working owner schema that changed since its handle opened.
Existing creation intents remain readable; malformed intents are preserved and
reported with their path, never silently discarded. These mechanisms coordinate
cooperating writers, not arbitrary external file mutation.

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
