---
type: design
summary: "Spec: cargo crate graph and forbidden scientific-authority edges"
last_validated: 2026-09-12
---

# Spec: Crate Graph

The workspace has four library crates and one binary crate. Dependency
direction is part of the scientific contract. An illegal edge is a
build failure, not a review comment.

## Why This Exists

Python could only state "importers never write" in prose. Cargo can refuse
the import.

## Members

| Crate | Role | May write scientific records? |
|-------|------|-------------------------------|
| `orbit-research-contract` | types, schemas, canonical JSON, validate, reconcile, science guards | no |
| `orbit-research-owner` | git checkout, native appends, artifact byte verify | yes, JSON appends under `research/` only |
| `orbit-research-import` | four owner adapters | no |
| `orbit-research-index` | sqlite projection, static export | no |
| `orbit-research-cli` | `[[bin]]` `orbit-research` | only by calling `owner` |

## Allowed edges

```
orbit-research-cli      → contract, owner, import, index
orbit-research-index    → contract, owner
orbit-research-import   → contract
orbit-research-owner    → contract
orbit-research-contract → (workspace-external crates only)
```

`index → owner` is allowed for **read** APIs. It is the known leaky
boundary; do not `pub use` write entry points from index-facing modules.

## Forbidden edges

These must fail `scripts/check-dependency-direction.sh`:

- `import` → `owner`
- `index` → `import`
- `contract` → `owner` | `import` | `index` | `cli`
- any workspace crate → `orbit-core` | `orbit-store` | `orbit-types` | `orbit-common` | `orbit-cli` | other `orbit-*` libraries

`orbit-research-cli` may spawn the `orbit` **executable**. That is not a
crate dependency.

## Failure modes

- Adding a helper to `owner` and depending on it from `import` "just for
  `safe_path`": forbidden. Duplicate or extract `orbit-research-git`.
- `contract` opening a schema file from disk at runtime: forbidden.
  Embed schema bytes.
- `index` calling `Owner::apply` to "repair" a document: forbidden.
- A published catch-all `orbit-research` crate that re-exports `Owner` next
  to `rebuild`: forbidden. Downstream owners choose `contract` or `owner`
  explicitly.

## Migration

If read-only git/path helpers force a sixth crate, add
`orbit-research-git` below `owner` and `import`, still above `contract`.
Do not relax a forbidden edge to avoid that crate.

See [Scientific crate graph is an authority boundary](../4_decisions.md#scientific-crate-graph-is-an-authority-boundary).

## Agent Signature

grok · 2026-09-12
