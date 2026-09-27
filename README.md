# orbit-research

A local research workbench for canonical Observatory Markdown records, with a
CLI and MCP interface. Orbit owns execution tasks, crews, runs and delivery
directly; scientific records stay in the selected knowledgebase.

The workbench is being converted into an Orbit plugin (see the constellation
`operations/research/orbit-research-plugin.md` spec). Local corpus operations
(capture, list, show, check, create, revise, planning) are available. Research
views become read-only Orbit plugin panels rather than a standalone dashboard;
task creation, dispatch and acceptance land through the plugin's `plan`/`link`/
`validate`/`accept` tools in later slices.

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

- [Architecture](ARCHITECTURE.md): the four crates and module ownership.
- [Workbench design](docs/design/research-workbench/1_overview.md): product scope
  (superseded in part; see the note at its top).
- [Research contracts](docs/design/research-workbench/specs/contracts.md): canonical
  records and owner boundaries (execution sections superseded; see the note at
  its top).
- [Dashboard design](docs/design/user-interface/1_overview.md): superseded — the
  standalone dashboard is removed; research views become Orbit plugin panels.
- [CLI design](docs/design/terminal-interface/1_overview.md).

The supported workflow uses canonical Observatory Markdown records. Use the CLI
or MCP server against an explicitly selected corpus; Orbit owns task creation,
dispatch and delivery through its own native commands.

## Terminal use

Run `orbit-research` or `orbit-research --help` for readable help and examples.
Output defaults to human tables on a terminal and headerless TSV lists in pipes.
Scripts that previously relied on default JSON must select `--format json` or set
`ORBIT_RESEARCH_FORMAT=json`; explicit JSON retains the canonical result shape.

```sh
orbit-research research list --corpus ./observatory
orbit-research research show --corpus ./observatory --id Q001
orbit-research --format json research list --corpus ./observatory
```

See the [terminal design](docs/design/terminal-interface/2_design.md) for output,
error and compatibility contracts.
