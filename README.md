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

`make install` builds a release CLI and installs it to `~/.local/bin`. Override
`INSTALL_BIN_DIR` for another destination or use `INSTALL_PROFILE=debug` for a
development build. `CARGO_TARGET_DIR` is respected. `BUILD_BUDGET` optionally names
a command wrapper accepting `-- COMMAND ...`; its default (`env`) runs Cargo directly.

## Design and structure

- [Architecture](ARCHITECTURE.md): the five crates and module ownership.
- [Workbench design](docs/design/research-workbench/1_overview.md): product scope.
- [Research contracts](docs/design/research-workbench/specs/contracts.md): canonical
  records, owner boundaries and execution integration.
- [Dashboard design](docs/design/user-interface/1_overview.md).
- [CLI design](docs/design/terminal-interface/1_overview.md).

The supported workflow uses canonical Observatory Markdown records. Use the CLI,
MCP server, or loopback dashboard against an explicitly selected corpus; connect
an Orbit backend only when linking or executing planned work.
