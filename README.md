# orbit-research

A local research workbench for canonical Observatory Markdown records, with a
separate dashboard, CLI and MCP interface. Orbit owns execution tasks, crews,
runs and delivery; scientific records stay in the selected knowledgebase.

The workbench redesign is under active implementation. Local corpus operations
are available; connected execution is gated by the bundled Orbit compatibility
allowlist. An uncertified backend is refused, without disabling local research.

## Build and validate

Requires Rust 1.89+ and Git.

```sh
cargo build --workspace --locked
make test
```

## Design and structure

- [Architecture](ARCHITECTURE.md): the six crates and module ownership.
- [Workbench design](docs/design/research-workbench/1_overview.md): product scope.
- [Research contracts](docs/design/research-workbench/specs/contracts.md): canonical
  records, owner boundaries and execution integration.
- [Dashboard design](docs/design/user-interface/1_overview.md).
- [CLI design](docs/design/terminal-interface/1_overview.md).

Legacy JSON authoring, imports and static export remain compatibility paths,
separate from the Markdown workbench. Their required schemas live in Common’s
`assets/schemas/`; synthetic fixture generators live in CLI’s
`tests/fixtures/legacy/`. They are not the new scientific-record contract.
Historical design notes under `docs/design/rust-port/` describe the prior port;
[Architecture](ARCHITECTURE.md) governs the current crate layout.
