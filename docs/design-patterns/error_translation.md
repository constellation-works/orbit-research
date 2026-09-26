---
type: pattern
summary: "Workspace Error Boundaries"
last_validated: 2026-09-25
---
# Workspace Error Boundaries

The research workspace shares one typed error surface from
`orbit-research-common`: `orbit_research_common::Error` and its `Result<T>`
alias. Store and Core propagate that error at their library boundaries; they do
not define separate crate error enums or translate every cross-crate error
through a local `*_error_to_*` function.

The shared error preserves common failure types and adds context when needed:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
}
```

Use `?` when a failure already maps to one of these variants. Convert to
`Error::Invalid` when the boundary adds useful context, such as identifying an
invalid owner schema or record path. Avoid stringifying errors in Core or Store
when the shared type can retain their source.

## Translate at external boundaries

Presentation and protocol adapters may map the shared error into their own
response shape. For example, the Web adapter converts it to an HTTP `Reply`,
while the CLI renders failures through its output layer. Keep those mappings at
the adapter boundary; they do not require each library crate to own a second
error hierarchy.

Common is a workspace leaf, so it cannot depend on Store, Core, CLI, or Web.
Keep shared error variants independent of application policy and transport
details. See the [workspace layering](../../ARCHITECTURE.md).
