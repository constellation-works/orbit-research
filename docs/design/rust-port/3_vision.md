---
title: Rust Port — Vision
owner: grok
last_updated: 2026-09-13
last_validated: 2026-09-13
status: Accepted
feature: rust-port
doc_role: vision
type: design
summary: After the drop-in Rust CLI, optional schema v3, owner-crate adoption, and git2 remain open; none are required to retire Python.
tags: [rust-port]
paths: ["crates/**"]
related_features: [rust-port]
related_artifacts: [ORB-12384, ORB-11353, ORB-11392]
---

# Rust Port — Vision

Forward-looking. The accepted port is a drop-in Rust CLI that preserves v1/v2
records. The questions below are not required to retire Python. Speculation
is labelled.

## 1. Open Questions

1. **Research-view pin cutover.** The Python tree was deleted in [ORB-12414]
   after the drop-in gate. Constellation still documents two historical Git
   pins of the Python package; updating `operations/research` is
   constellation-owned ([ORB-12415]).
2. **Schema v3 / RFC 8785.** A clean canonicalization would remove the
   CPython byte-matching module. It would also rewrite every `revision_id`.
   Not in this port. If it happens, v1/v2 stay read-only historical encodings.
3. **Owner crates depending on Rust.** Principia, Parallax, Orrery and
   Astrolabe could call `orbit-research-contract` instead of installing
   Python. That is owning-repository work, not this repo.
4. **`git2` after the pin oracle is green.** Once subprocess fixtures lock
   blob/OID/dirty-tree behavior, a `Git` trait impl over `git2` may be
   faster and easier to sandbox. It is an internal swap, not a contract
   change.
5. **Orbit plugin packaging.** orbit-graph ships `plugin/*.orbit-tool.yaml`.
   `resource --version 1` already prints a packaged SKILL.md. A later plugin
   could expose `validate` / `index` as Orbit tools. It must not grow a
   second task engine.

## 2. Prior Work

### This repository (Python)

The scientific contract, native appends, importers, and index/browser are
already specified in `docs/contract.md`, `docs/native-workflow.md`,
`docs/imports.md`, and `docs/browser.md`. The Rust port re-implements those
documents; it does not reopen them.

### Constellation Rust crates

- **orbit** — layered workspace, crate-edge contract script, `jsonschema`
  0.18, bundled `rusqlite`, newtype-at-the-boundary, fail-closed defaults.
  Copy the layering *principle* and the lint/MSRV conventions. Do not copy
  the twenty-crate shape.
- **orbit-graph** — library crate plus `[[bin]]`, explorer as a second
  product, plugin YAML, Makefile CI. Closest size analog, except
  orbit-research has a real authority split (contract vs write vs import vs
  projection) that orbit-graph does not.
- **nebula** — single binary crate. Too small a split for this port because
  owning repos need to depend on validation without SQLite.

### Canonical JSON

RFC 8785 / JCS is the obvious standard and is **not** what v1/v2 records
hash. Python `json.dumps` with `sort_keys=True`, compact separators, and
`ensure_ascii=False` is the advertised encoding. Number representation and
Unicode normalization are not rewritten.

## 3. What May Be Distinctive

Nothing in the scientific meaning. The distinctive move is using the Cargo
crate graph as the enforcement mechanism for "importers do not write" and
"the index is not a store," which Python could only state in prose and
tests.

## 4. References

**Orbit-internal**

- [2_design.md](./2_design.md)
- [4_decisions.md](./4_decisions.md)
- orbit repository `docs/design/CONVENTIONS.md`
- orbit repository `ARCHITECTURE.md` (crate layering, not crate count)
- orbit-graph `docs/design/` (library plus product split)

**External**

- JSON Schema Draft 2020-12
- RFC 8785 (explicitly *not* the v1/v2 encoding)

## Task References

- [ORB-12384] — recorded this rust-port design folder
- [ORB-11353] — approved the starting scientific contract for the Python package
- [ORB-11392] — added the disposable index and static browser (package 0.3)

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
