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

> **Partially superseded (2026-09-27):** the "Orbit backend" and "Result
> acceptance" sections below describe the CLI adapter and agent-supplied
> receipt model, both removed. Orbit owns dispatch/status/cancel through its
> own native commands. `validate` is the plugin's delivery gate (see "Delivery
> gate" below); `accept` returns as a plugin tool in a later slice. See the
> constellation `operations/research/orbit-research-plugin.md` spec. The
> canonical corpus and writes/recovery sections are unaffected.

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

Every record write goes through the `orbit-research` writer in one of two modes,
detected from the checkout and reported as `mode` in each result. Primary mode
(the primary checkout) is the guarded writer below: clean tree, exclusive lock,
durable intent, expected blobs and a commit. Only primary mode allocates IDs;
`create --kind R --status planned` is the reservation, and `capture` records a
question from text and tags alone. Worktree mode (a linked run worktree) writes
only the run's reserved R — its README and `data/manifest.json` — after checking
them against the owner schema and the rest of the corpus. It never allocates IDs
and never commits; the run's commit step publishes the files. The reserved R
must already exist at the worktree's HEAD as a `planned` or `running` stub, and
the worktree's first write binds it to that R. Allocation, commits, and writes to
any other record refuse with a typed refusal. A caller may assert the mode it
expects; a mismatch refuses.

`revise` and `revise_question` edit Q/H/T in primary mode under an expected blob.
A field the caller omits keeps its current value; an empty tag list clears the
tags, and an empty hypothesis or theory body is refused. A revision that leaves
the record as it was makes no commit and reports `changed: false`. Identity, path,
lineage and assessments stay frozen. A hypothesis title or body change bumps its
`revision` and reopens its status; earlier assessments stay on their revision.
`assess` appends `{date, research, revision, verdict, strength, note}` to a
hypothesis. It refuses a revision the hypothesis never had, a citation that is
not an R, and an R whose acceptance cannot be verified: the R has no
`orbit.task`, its task has no `research-acceptance.json`, the artifact is
unreadable or names another record, the README blob differs from the accepted
one, or Orbit cannot be reached. Entries are never rewritten or
reordered. For the current revision, status follows the owner schema's
`verdict_status`, except that a dropped hypothesis stays dropped. The verdict is
always the author's; neither execution success nor acceptance supplies it.

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

Task linking stores correlation before submission. On Unix, a successful request-log
save syncs the published entry's directory, including the log directory's own
parent when it was newly created, before Orbit task creation can begin. Retry
queries the exact Orbit correlation even when the local intent entry is absent,
and adopts exactly one match; zero after an unknown outcome or multiple matches
is an explicit unresolved state. Dispatch observes existing task/run correlation
before any new submission. Status reads are fresh Orbit observations with
timestamp/source.
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

Expose validated reads, Q/H/T/R creation, capture, guarded revision, assessment,
task drafting (`plan`: investigation, contribution or synthesis) and local request
correlation through Core. CLI/MCP share semantics and failure behavior. Read and
planning operations never dispatch implicitly. Tool schemas disallow unrecognized
fields; corpus scope is fixed during process composition. Machine stdout contains
protocol/output only. Task linking is the plugin's own `link` tool: it stores
correlation before submission through the `orbit.task.add`/`orbit.task.list`
callbacks, and Core itself never shells out to Orbit. Dispatch, status,
cancellation move to Orbit-native commands. The plugin's `validate` tool gates
delivery; result acceptance moves to its `accept` tool (a later slice; see the
constellation `operations/research/orbit-research-plugin.md` spec).

## Delivery gate

The plugin's `research_investigation` job runs `worktree_setup`, the
`research_investigate` agent, `validate`, `git_commit`, `git_merge` and
`update_task` to `review`. `validate` runs as a deterministic
`plugin.tool_call` with no recovery activity, so an invalid record fails the run
before commit.

`validate` reads only the checkout named by its `path` input, never the bound
workspace. A job step's `context.workspace_root` is the primary checkout even
while the run's work is in its worktree. The record is the optional
`research_id` input, or else the one the worktree-mode writer bound the worktree
to. The record must name `context.task_id` and `context.job_run_id` in
`orbit.task` and `orbit.run`; without that run context the call refuses with
`run_context_required`. Against the checkout's HEAD, the gate requires:

- the record parses and passes the owner schema, ID and slug rules;
- the owner's README sections (`## Question`, `## Method`, `## Result`,
  `## Limitations`, `## Next`) are present in order, each non-empty and not
  still holding the reserved stub's text;
- `data/manifest.json` passes the owner schema;
- every input with a declared `sha256` whose bytes are at `data/<name>` matches
  that digest and any declared `size`. Absent bytes, which are never committed,
  are reported as `unverified_inputs` and do not fail the gate;
- every reference target exists in the checkout;
- no record path is absent from HEAD (no allocated ID), no other record changed
  or disappeared, and the whole corpus passes the checker's rules.

Any finding replies `ok:false`. The error code is the first finding's reason
(`research_unbound`, `record_invalid`, `section_missing`,
`section_placeholder`, `orbit_task_mismatch`, `orbit_run_mismatch`,
`manifest_invalid`, `artifact_digest_mismatch`, `dangling_lineage`,
`id_allocated`, `other_record_changed` or `corpus_invalid`), and the message
lists every finding. Orbit fails a step only on `ok:false`, not on
`valid:false` output. The gate reads Git in-process and spawns nothing, so it
runs under the plugin sandbox.
