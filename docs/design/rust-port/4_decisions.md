---
title: Rust Port — Decisions
owner: grok
last_updated: 2026-09-12
last_validated: 2026-09-12
status: Accepted
feature: rust-port
doc_role: decisions
type: design
summary: Standing rules for the Rust port — crate-graph authority, Python-compatible digests, subprocess git, no Orbit linkage, disposable projections, fail-closed defaults.
tags: [rust-port]
paths: ["crates/**"]
related_features: [rust-port]
related_artifacts: [ORB-12384, ORB-11353, ORB-11391, ORB-11392]
---

# Rust Port — Decisions

Record non-obvious decisions here by title. Task references carry provenance;
superseded decisions remain in place so their original reasoning stays
legible. See [CONVENTIONS.md §4](../CONVENTIONS.md) and the orbit repository
`docs/design/CONVENTIONS.md` §4 for the admission rule and required `Cost:`
line.

Crate names, slice order, and "four crates not twenty" are design prose in
[2_design.md](./2_design.md). They do not earn entries.

## Scientific crate graph is an authority boundary

**Recorded:** 2026-09 · [ORB-12384]

### Context

Python encoded "importers never write records" and "the index is not a store"
in comments, AGENTS.md, and tests. A later contributor can still `from
.native import Owner` inside `importers.py`. The same class of mistake in
Rust is a `Cargo.toml` dependency. New adapters, new projection features, and
new owner operations will keep hitting this tradeoff.

### Decision

Treat the crate graph as the scientific authority boundary. A crate that is
not allowed to write records must not depend on the crate that writes them. A
crate that is not a scientific store must not grow APIs that persist owner
JSON. When a convenient helper would require an illegal edge, duplicate the
thin helper or extract a read-only crate rather than relax the edge.

### Consequences

- Tomorrow's fifth adapter still cannot call `Owner::apply` without a
  workspace-level dependency change that review will see.
- Shared git/path helpers are the recurring tax; a sixth `orbit-research-git`
  crate is the escape hatch, not a silent `import → owner` edge.
- Cost: some code (safe_path, git_bytes, strict JSON file reads) will be
  duplicated or extracted later instead of living once next to `Owner`.

## Python-compatible canonical JSON until a schema version bump

**Recorded:** 2026-09 · [ORB-12384]

### Context

v1/v2 `revision_id` and `protocol_digest` are SHA-256 over CPython
`json.dumps(..., sort_keys=True, separators=(',', ':'), ensure_ascii=False)`
bytes. That encoding is not RFC 8785. Any future digest — a new record kind,
a new native field, a hashed export — will face the same choice: match
Python, or bump the schema and rewrite history.

### Decision

Every digest over a `schema_version` 1 or 2 document uses the Python
canonical byte encoding. RFC 8785, `serde_json` pretty/compact defaults, and
Unicode normalization are forbidden for those versions. A different encoding
requires a new `schema_version` and leaves v1/v2 bytes unchanged.

### Consequences

- Existing owner records and research-view pins keep their IDs across the
  language change.
- Cost: the project carries a CPython-matching canonicalizer, including
  number formatting and `legacy` key order, instead of a one-line RFC 8785
  call.

## Git as a subprocess until pin semantics are proven

**Recorded:** 2026-09 · [ORB-12384]

### Context

Provenance pins are "what `git show` / `rev-parse` / `ls-tree` returned,"
including null HEAD, dirty worktrees, and blob OIDs. `git2` is the
constellation default for repository reads. The two do not always agree on
partial clones, SHA-256 repos, or lock behavior (`GIT_OPTIONAL_LOCKS=0`).

### Decision

Talk to Git through an explicit subprocess wrapper with
`GIT_OPTIONAL_LOCKS=0` until a fixture suite locks blob, OID, dirty-tree,
and missing-path behavior. Hide the wrapper behind a `Git` trait. A later
`git2` implementation is allowed only if it matches those fixtures.

### Consequences

- Pin semantics stay comparable to the Python package during dual-run.
- Cost: process spawn per `git show`, and no in-process object database,
  until someone pays for a proven `git2` impl.

## Do not link Orbit crates

**Recorded:** 2026-09 · [ORB-12384]

### Context

`task-context` already shells out to `orbit tool run orbit.task.show`. Linking
`orbit-core` / `orbit-store` would make scientific records compile against
operational types, and would pull Orbit's workspace, config, and store into
a package whose AGENTS.md forbids a second task engine. Any future "just
read the task from the store" feature will look cheaper than the subprocess.

### Decision

This workspace does not depend on `orbit-core`, `orbit-store`, `orbit-types`,
or any other Orbit library crate. Operational identifiers (`host`,
`workspace`, `task`, `run`) are opaque strings on `orbit_links`. Assigned-task
reads go through the operator-selected `orbit` executable.

### Consequences

- Scientific meaning stays independent of Orbit releases and store layout.
- Cost: no type-level guarantee that a `task` id is well-formed; a store
  schema change is discovered at subprocess time, not compile time.

## SQLite projections are disposable and may not write records

**Recorded:** 2026-09 · [ORB-12384]

### Context

The index is the feature most likely to accrete authority: a convenient
"repair this pin," a cached native append, a browser edit. Python already
forbade that in prose. Every later projection feature (new columns, media
tables, incremental rebuild) has to decide whether the database is allowed
to persist owner meaning.

### Decision

SQLite in this workspace is a disposable projection rebuilt from owner
documents. It may not grow APIs that create, update, or delete scientific
records, manifests, or import reports. Rebuild is atomic and fail-closed.
Derived reconciliation lives on the projection only.

### Consequences

- Losing the database is an inconvenience, not data loss.
- Cost: every browser or query change pays a full rebuild, and "just patch
  the row" is never a legal fix for a bad owner document.

## Fail closed rather than degrade silently

**Recorded:** 2026-09 · [ORB-12384]

### Context

The Python package already fails closed on dirty trees, revision mismatch,
duplicate JSON keys, non-finite numbers, symlink escapes, and strengthened
historical verdicts. A Rust port will be tempted to "be helpful": skip a
bad document, coerce `NaN`, fall back to HEAD, or treat a successful
process as evidence. That temptation will recur at every new adapter,
schema field, and CLI flag.

### Decision

Unknown, dirty, ambiguous, or unverified meaning is explicit missingness or
a hard error. Do not guess, coerce, or fall back to a convenient snapshot.
A successful subprocess is not scientific support. A pending reference is
valid data and is never current confirmatory evidence.

### Consequences

- Operators see exceptions instead of silently wrong inventories.
- Cost: dry-runs and index builds will refuse work that a looser tool could
  partially render; that refusal is the product.

## Task References

- [ORB-12384] — recorded this rust-port design folder
- [ORB-11353] — approved the starting scientific contract for the Python package
- [ORB-11391] — added native authoring and record schema v2
- [ORB-11392] — added the disposable index and static browser (package 0.3)

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
