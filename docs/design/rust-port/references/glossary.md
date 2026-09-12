---
type: design
summary: "Glossary: Rust Port"
last_validated: 2026-09-12
---

# Glossary: Rust Port

Vocabulary this feature gives a specific meaning. Ordinary terms (crate,
digest, blob, SQLite) stay out unless the port uses them in a narrower way.

| Term | Meaning |
|------|---------|
| **Canonical JSON (v1/v2)** | CPython `json.dumps` with `sort_keys=True`, compact separators, `ensure_ascii=False`, finite numbers only. Not RFC 8785. [specs/canonical-json.md](../specs/canonical-json.md). |
| **Drop-in gate** | The evidence bar that must pass before the Rust `orbit-research` binary may replace a Python pin in research-view. [2_design.md §7](../2_design.md). |
| **Forbidden edge** | A Cargo dependency the crate graph must not grow, because it would let a crate exercise scientific authority it does not own. [specs/crate-graph.md](../specs/crate-graph.md). |
| **Owner store** | Append-only JSON under `research/records` in an explicit Git checkout. Git publishes; Orbit executes; this crate only writes JSON. [2_design.md §3](../2_design.md). |
| **Pin** | Exact tuple `(repository, id, revision_id, source_revision)`. Identity for reconcile, index, and confirmation. [2_design.md §2](../2_design.md). |
| **Projection** | Disposable SQLite (and the static export built from it). Rebuilt from owner documents; never a second scientific store. [2_design.md §5](../2_design.md). |
| **Scientific contract** | The six record kinds plus validate/reconcile/digest/confirmation rules, independent of git, sqlite, and Orbit. [2_design.md §2](../2_design.md). |
