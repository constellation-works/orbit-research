---
title: Rust Port — Overview
owner: grok
last_updated: 2026-09-13
last_validated: 2026-09-13
status: Accepted
feature: rust-port
doc_role: overview
type: design
summary: Replace the Python orbit-research package with a Rust workspace whose crate graph encodes scientific authority, keeping the existing CLI and digest contract.
tags: [rust-port, crates, scientific-contract]
paths: ["crates/orbit-research-contract/**", "crates/orbit-research-owner/**", "crates/orbit-research-import/**", "crates/orbit-research-index/**", "crates/orbit-research-cli/**"]
related_features: [rust-port]
related_artifacts: [ORB-12384, ORB-11353, ORB-11367, ORB-11391, ORB-11392, ORB-12389, ORB-12414]
---

# Rust Port — Overview

`orbit-research` is the shared scientific registry for Principia, Parallax,
Orrery and Astrolabe: immutable native authoring, conservative read-only
imports, a disposable SQLite projection, and a local static evidence browser.
The running implementation is the Rust workspace. This feature ported the
Python 0.3 package without changing the scientific contract, the JSON CLI, or
the rule that owning repositories remain the authority for records.

## 1. Motivation

The Python package is small (~2.1k lines of library code) and already layered,
but it is the wrong language for the rest of the constellation's tooling
surface. Orbit, orbit-graph and nebula are Rust. A Rust `orbit-research`
binary can share their edition, MSRV, lints, `rusqlite` and `jsonschema`
conventions, and can be depended on by owner checkers without a Python
environment.

The port is not a rewrite of meaning. Package milestones [ORB-11353],
[ORB-11367], [ORB-11391] and [ORB-11392] already fixed the record kinds,
native append rules, import preservation, and index/browser seam. The job is
to encode those seams as crate boundaries so an adapter cannot write records
and an index cannot become a second store.

## 2. Core Concepts

- **Scientific contract** — the six record kinds, canonical JSON, digests,
  `validate` / `reconcile`, and native confirmation guards. No git, sqlite, or
  Orbit. See [2_design.md §2](./2_design.md).
- **Owner store** — append-only JSON under `research/records` in an explicit
  Git checkout. Git supplies publication pins; Orbit executes tasks. See
  [2_design.md §3](./2_design.md).
- **Read-only import** — four owner adapters that emit v1 candidate reports.
  Never writes a scientific record. See [2_design.md §4](./2_design.md).
- **Disposable projection** — SQLite index plus static HTML export, rebuilt
  from owner documents. Not a scientific store. See [2_design.md §5](./2_design.md).
- **Drop-in CLI** — JSON-only `orbit-research` with the same subcommands and
  exit codes the research-view launcher already pins. See
  [specs/cli-compat.md](./specs/cli-compat.md).
- **Python-compatible canonical JSON** — v1/v2 `revision_id` bytes match
  CPython `json.dumps(sort_keys=True, separators=(',', ':'), ensure_ascii=False)`.
  Not RFC 8785. See [specs/canonical-json.md](./specs/canonical-json.md).

## 3. At a Glance

| Concern | File | Task |
|---------|------|------|
| Folder layout | [../CONVENTIONS.md](../CONVENTIONS.md) | [ORB-12384] |
| Crate graph and forbidden edges | [2_design.md §1](./2_design.md), [specs/crate-graph.md](./specs/crate-graph.md) | — |
| Types, schemas, science invariants | [2_design.md §2](./2_design.md) | — |
| Python-compatible canonical JSON | [specs/canonical-json.md](./specs/canonical-json.md), [Python-compatible canonical JSON until a schema version bump](./4_decisions.md#python-compatible-canonical-json-until-a-schema-version-bump) | — |
| Native owner appends and artifact verify | [2_design.md §3](./2_design.md) | — |
| Read-only owner adapters | [2_design.md §4](./2_design.md) | — |
| SQLite index and static browser | [2_design.md §5](./2_design.md) | — |
| CLI compatibility | [2_design.md §6](./2_design.md), [specs/cli-compat.md](./specs/cli-compat.md) | — |
| Scientific crate graph as authority boundary | [Scientific crate graph is an authority boundary](./4_decisions.md#scientific-crate-graph-is-an-authority-boundary) | — |
| No Orbit crate linkage | [Do not link Orbit crates](./4_decisions.md#do-not-link-orbit-crates) | — |
| Git as subprocess | [Git as a subprocess until pin semantics are proven](./4_decisions.md#git-as-a-subprocess-until-pin-semantics-are-proven) | — |
| Python deleted after the drop-in gate | [2_design.md §7](./2_design.md) | [ORB-12414] |

## Task References

- [ORB-12384] — recorded this rust-port design folder
- [ORB-11353] — approved the starting scientific contract for the Python package
- [ORB-11367] — landed the first installable Python milestone
- [ORB-11391] — added native authoring and record schema v2
- [ORB-11392] — added the disposable index and static browser (package 0.3)
- [ORB-12389] — proved the Rust CLI is a drop-in for Python 0.3
- [ORB-12414] — deleted the Python package after the drop-in gate

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
