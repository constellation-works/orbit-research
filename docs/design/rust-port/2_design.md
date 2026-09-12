---
title: Rust Port — Design
owner: grok
last_updated: 2026-09-12
last_validated: 2026-09-12
status: Accepted
feature: rust-port
doc_role: design
type: design
summary: Four library crates plus one CLI binary; Python stays until the Rust binary matches v1/v2 digests and the existing JSON CLI.
tags: [rust-port, crates, canonical-json]
paths: ["crates/**", "schemas/**", "resources/**", "web/**"]
related_features: [rust-port]
related_artifacts: [ORB-12384, ORB-11353, ORB-11367, ORB-11391, ORB-11392]
---

# Rust Port — Design

This document specifies the Rust workspace that will replace the Python
package. Python remains the running implementation until the drop-in gate in
§7. Forward-looking deletions, schema v3, and owner-crate adoption live in
[3_vision.md](./3_vision.md). Standing rules that govern later tradeoffs live
in [4_decisions.md](./4_decisions.md).

The Python modules are the semantic source. Approximate sizes at design time:
`importers.py` 471, `index.py` 389, `native.py` 359, `contract.py` 317,
`science.py` 225, `cli.py` 141, `artifacts.py` 103, `browser.py` 93,
`orbit_path.py` 30. That is not enough code to copy Orbit's twenty-crate
layout. The split below exists to make illegal authority edges fail at
compile time, not to mirror Orbit.

## 1. Crate graph

```
orbit-research/                  # cargo workspace
  crates/
    orbit-research-contract/     # scientific JSON contract; no I/O
    orbit-research-owner/        # git checkout, appends, artifact verify
    orbit-research-import/       # four read-only adapters
    orbit-research-index/        # sqlite projection + static export
    orbit-research-cli/          # [[bin]] name = "orbit-research"
  schemas/v1/ v2/                # interchange source of truth
  resources/v1/SKILL.md
  web/                           # static browser assets
```

Dependency direction:

```
cli → owner, import, index, contract
index → owner, contract
import → contract
owner → contract
```

Forbidden edges, checked by `scripts/check-dependency-direction.sh` once the
workspace exists (same idea as Orbit's crate-edge contract):

- `import` does not depend on `owner` — adapters cannot call `Owner::apply`
- `index` does not depend on `import` — projection never runs an adapter
- `contract` depends on nothing in this workspace — no git, sqlite, subprocess, or filesystem except embedded schema bytes
- no crate depends on an `orbit-*` library crate — `task-context` stays a CLI subprocess

`index` may depend on `owner` only for **read** APIs (`NativeReceipt::verify_chain`,
`git_bytes`, `Checkout`). Do not expose a convenience `Owner::apply` on types
the index constructs. If a later change needs compile-time separation of
read vs write, extract `orbit-research-git`; do not start there.

Public crate names stay `orbit-research-*`. Do not publish a catch-all
`orbit-research` lib that re-exports writes. Owning repositories should be
able to depend on `orbit-research-contract` (and maybe `owner`) without
taking SQLite or the four adapters.

Prescriptive edge list: [specs/crate-graph.md](./specs/crate-graph.md).
Standing rule: [Scientific crate graph is an authority boundary](./4_decisions.md#scientific-crate-graph-is-an-authority-boundary).

Workspace conventions match orbit-graph: edition 2024, rust-version 1.89,
workspace clippy lints (`unwrap_used` warn, `dbg_macro` deny, `print_stdout`
warn), `thiserror`, `serde`, `clap` with derive, bundled `rusqlite`,
`jsonschema` 0.18 without default features (same crate Orbit uses).

## 2. Contract crate

`orbit-research-contract` is the product. Everything else is I/O around it.
It absorbs today's `contract.py` and `science.py`; that Python split was
files, not a dependency boundary. `validate()` already calls `native_errors`
and `confirmation_errors` for schema v2.

### 2.1 Newtypes and enums

Parse at the boundary, then trust the type (Orbit's newtype pattern):

- `RepositoryId` — `[a-z0-9][a-z0-9.-]*`
- `RecordUrn` — `urn:research:<repo>:<kind>:<percent-encoded-id>` with encoding matching `urllib.parse.quote(safe="")`
- `RevisionId` / `Digest` — `sha256:` plus 64 lowercase hex
- `GitRevision` — 40 or 64 lowercase hex, or explicit null missingness
- `Pin` — `(repository, id, revision_id, source_revision)` as one value; index, reconcile, and confirmation all key on this
- `Reference` with `status: Pending | Resolved`

Enums, not strings, for `Kind`, `Activity`, `Scope`, `Verdict`, `Inference`,
`Freeze`, `ExecutionStatus`, `EvidenceSummary`. Unknown envelope fields are
rejected. `legacy` stays a `serde_json::Value` bag so importers do not guess
meaning.

### 2.2 Schemas and validation

JSON Schema Draft 2020-12 files under `schemas/v1/` and `schemas/v2/` remain
the interchange source of truth. The crate embeds them and registers them
locally; it never fetches schema URLs. Structural validation uses the
`jsonschema` crate. Scientific invariants (digest match, historical freeze
cannot claim prospective preregistration, execution is not support, model
scope cannot confirm a claim about nature, confirmatory-primary closure)
live in the same crate as pure functions over already-parsed records.

`ArtifactResolver` is a trait only. Filesystem verification does not live
here:

```rust
pub trait ArtifactResolver {
    fn verify(&self, record: &Record, r#ref: &Reference)
        -> Result<Option<VerifiedPin>, ContractError>;
}
```

Default reconcile does no file or network lookup. A pending reference is
valid missingness and is never current confirmatory evidence.

### 2.3 Canonical JSON and digests

`protocol_digest` hashes only `payload.semantic`. Non-protocol `revision_id`
hashes the record excluding `revision_id`, `presentation`, and `provenance`.
The byte encoding must match CPython for every existing v1/v2 record. That
module is specified in [specs/canonical-json.md](./specs/canonical-json.md).
Until golden tests against `examples/migration-report.json` and live owner
exports pass, the Rust CLI is not a substitute for the Python 0.3 pin.

## 3. Owner crate

Today's `native.py` plus `artifacts.py` plus shared path/git helpers.

- `Checkout` — explicit git toplevel, `GIT_OPTIONAL_LOCKS=0`, `git show <rev>:<path>`, blob OID, no HEAD fallback on resolve.
- `Owner::apply` for `program | claim | artifact | preregister | begin-run | record-run | assess | retire`.
- Directory inode flock, `NNNNNNNN-<digest>.json` sequence, idempotent `request_id`.
- `heads` / `pin` / `trace` / `export` / `NativeReceipt::verify_chain`.
- Concrete `FsArtifactResolver`.

Git is a subprocess wrapper first, not `libgit2`. The current contract is
what `git show` / `rev-parse` / `ls-tree` returned, including null pins and
dirty-tree detection. Hide the wrapper behind a `Git` trait so contract tests
do not need real repositories. See
[Git as a subprocess until pin semantics are proven](./4_decisions.md#git-as-a-subprocess-until-pin-semantics-are-proven).

Canonical records stay under `research/` in the owning checkout. The crate
authors JSON; it does not commit, push, or talk to an Orbit store.

## 4. Import crate

Today's largest file, kept isolated.

- `Adapter: Principia | Parallax | Orrery | Astrolabe`
- default discovery patterns from `docs/imports.md`
- inventory and exception accounting, SQLite sidecar snapshots copied to a temp dir (never opened in place)
- `--expect-revision` fails closed
- emits a v1 `ImportReport` only
- `write_report` refuses paths inside the source root, existing files, hardlinks, and escaping symlinks

Depends on `contract` plus a small re-export of read-only git/path helpers
from `owner` **or** duplicated thin helpers that do not include `Owner::apply`.
The crate graph forbids a dependency on `owner` so the forbidden call is not
even nameable. If the thin helpers grow, extract `orbit-research-git`.

Unknown meaning is an exception, not a guessed fact. Historical imports always
use `inference: historical` and `basis: legacy-report`. The importer never
strengthens a verdict, invents preregistration, or deletes anything.

## 5. Index crate

Package 0.3, still a projection. Absorbs `index.py` and `browser.py` (93
lines — not a fifth crate).

- operator-authored index config, explicit checkouts and document paths, no recursive whole-machine discovery
- atomic SQLite rebuild (lock + write-temp-rename), fail-closed `IndexBuildError`
- derived reconciliation copied onto a private projection; owner JSON is never edited
- `index-trace` for an exact indexed snapshot including assessments
- `browse-export` embeds `web/index.html|app.js|style.css`, copies raster-only media after digest check, maps local checkout URLs

No HTTP server in this crate. Serving remains ordinary static serving, as
`docs/browser.md` requires. `tiny_http` stays out.

The index may ingest validated owner manifests, native v2 exports, and
read-only import reports. At least one manifest is required. Competing
eligible assessments do not silently adjudicate one another.

## 6. CLI crate

Thin `clap` binary, JSON-only stdout and stderr, same subcommands as
`src/orbit_research/cli.py`:

`index`, `browse-export`, `index-trace`, `resource`, `task-context`,
`validate`, `import`, owner operations (`program`, `claim`, `artifact`,
`preregister`, `begin-run`, `record-run`, `assess`, `retire`, `heads`,
`ref`, `trace`, `export`), `reconcile`.

Exit codes: `0` success, `1` validation errors, `2` invalid-input. Errors
are `{"error":{"code":"...","message":"..."}}` on stderr.

`task-context` and `resource --version 1` fold in here (30 lines of Python,
not a crate). `task-context` runs `orbit tool run orbit.task.show` with
explicit `--orbit-root/host/workspace/task/run`. It never infers authority
from cwd and never links an Orbit library. See
[Do not link Orbit crates](./4_decisions.md#do-not-link-orbit-crates).

Prescriptive CLI contract: [specs/cli-compat.md](./specs/cli-compat.md).

## 7. Drop-in gate and Python retention

`operations/research-view.sh` pins two Git revisions of this package: native
authoring at 0.2.0 and the browser at 0.3.0. The Rust binary is a substitute
only when:

1. `revision_id` / `protocol_digest` golden tests match existing fixtures and at least one live owner export
2. `orbit-research validate` / `reconcile` round-trip the Python test corpus
3. native fixture (`examples/native_workflow.py` equivalent) produces structurally equal appends, traces and exports
4. import dry-runs on the synthetic four-owner fixtures match counts, exception classes, and candidate IDs
5. `index` + `browse-export` on the browser fixture match the Python projection digest for records and media

Python stays in-tree until that gate is green and the research-view pins are
cut over by a separate owner task. This repository does not edit sibling
owners or the constellation launcher from a framework task.

## 8. Implementation slices

Order is dependency order, not preference:

1. Workspace scaffold, lints, `check-dependency-direction.sh`, empty crates
2. `orbit-research-contract`: types, embedded schemas, canonical JSON, `validate` / `reconcile`
3. CLI `validate` and `reconcile` only
4. `orbit-research-owner` + CLI authoring commands
5. `orbit-research-import` + CLI `import`
6. `orbit-research-index` + CLI `index` / `index-trace` / `browse-export`
7. `task-context` / `resource`, Makefile/CI parity with orbit-graph, drop-in evidence against Python

Each slice must add focused behavioral tests for the scientific invariants it
touches. Do not wait for a final "port the test suite" task.

## 9. Concerns & Honest Limitations

- Canonical JSON is the load-bearing risk. `serde_json` will not match CPython
  number formatting, Unicode, or `legacy` key order by default. A plausible
  "it validates" port can still rewrite every `revision_id`.
- `index` depending on `owner` is a leaky read/write boundary. The design
  accepts it to avoid a sixth crate; a careless `pub use Owner` would let the
  projection write records. Review that surface on every owner-crate change.
- Percent-encoding must match Python `quote(safe="")` exactly, including
  literal `%`, `:`, Unicode and `/`. A Rust `urlencoding` crate with different
  defaults will split identities.
- Git subprocess behavior on dirty trees, missing blobs, and SHA-1 vs SHA-256
  repos is part of the scientific pin. Tests need real git fixtures, not mocks
  of happy-path `git show`.
- Dual-running Python and Rust in one repo will confuse installers until the
  drop-in gate. Keep `pyproject.toml` as the published 0.3 interface until
  cutover; do not ship a mixed-language package.
- This design does not migrate sibling owners, does not add a hosted browser,
  and does not make the index authoritative. Those remain out of scope even if
  they would make a demo easier.

## Task References

- [ORB-12384] — recorded this rust-port design folder
- [ORB-11353] — approved the starting scientific contract for the Python package
- [ORB-11367] — landed the first installable Python milestone
- [ORB-11391] — added native authoring and record schema v2
- [ORB-11392] — added the disposable index and static browser (package 0.3)

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
