---
type: pattern
summary: "Command Pattern"
last_validated: 2026-09-25
---
# Command Pattern

Represent an operation as a value that names the requested work, then route it
through a stable dispatcher. This separates callers from operation-specific
behavior and keeps discovery aligned with execution.

## Current application operation registry

The application layer defines its external research operations in
`crates/orbit-research-core/src/application/operation.rs`. The `operations!`
registry generates an `Operation` enum, parses wire names with `FromStr`,
builds each operation's request schema, and dispatches its typed request to a
handler. Each entry binds an external name, request type, handler, and
description.

The workspace uses this enum-based registry rather than a `Tool` trait
implementation per operation. The architecture document describes the
registry's ownership and transport boundaries in [ARCHITECTURE.md](../../ARCHITECTURE.md).

## Adding an operation

- Define its request type in `application/request.rs`.
- Implement its application handler in `application/operations.rs`.
- Add the name, request type, handler, and description to the registry in
  `application/operation.rs`.
- Keep transport framing in the CLI or Web adapter; both use the shared Core
  operation contracts.

This keeps operation schemas and dispatch derived from the same request types.
