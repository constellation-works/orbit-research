# orbit-research

A local research workbench for canonical Observatory Markdown records
(questions, hypotheses, theories, results), with a CLI, an MCP server and an
Orbit plugin. Records stay in the selected corpus; Orbit owns tasks, crews,
runs and delivery.

## The research loop

1. Capture a question, state a hypothesis and reserve a result (R) with the CLI or MCP.
2. `plan` drafts the task and `link` creates it in Orbit, scoped to that R.
3. `orbit run job research_investigation --input task=<task-id>` investigates in
   its own worktree, and its `validate` step blocks an invalid record before commit.
4. `accept` stores `research-acceptance.json` on the task once delivery lands.
5. `research assess` records the verdict only against an acceptance that matches
   the README at HEAD, and updates the hypothesis status.

The plugin's [`orbit-research-native` skill](.orbit-plugin/skills/native/SKILL.md)
walks through the loop and its refusals. [ARCHITECTURE.md](ARCHITECTURE.md) has the contracts.

## Orbit plugin

The plugin root is `.orbit-plugin/` and its backend is `orbit-research orbit-tool`.

- Read-only tools: `list`, `show`, `check`, `version`, `plan` and `validate`.
- Mutating tools: `link` and `accept`. They can write only the corpus's
  `_data/orbit-research-operations/` and `.orbit-research-tmp/`.
- The `research_investigation` job.
- Four read-only dashboard panels, on Orbit's Plugins tab:

| Panel | Shows |
|---|---|
| Open questions | open questions, their tags and linked tasks |
| Results awaiting acceptance | delivered results not yet accepted, changed since acceptance, or whose acceptance can't be checked right now |
| Hypotheses and assessments | each hypothesis revision with its latest verdicts; when results disagree, the revision is marked disputed |
| Corpus health | validity, base revision, counts by kind and status |

The plugin reads the workspace root as the corpus. Give each corpus its own new
directory (`orbit-research workspace init <dir>`) and register that directory as
an Orbit workspace. Panels never write records, and each one shows at most 200 rows.

## Build and install

Requires Rust 1.89+ and Git.

```sh
cargo build --workspace --locked
make test        # required gate
make install     # release build to ~/.local/bin (INSTALL_BIN_DIR, INSTALL_PROFILE=debug, BUILD_BUDGET)
```

The plugin runs only the binary bundled next to its launcher. Bundle it before
`orbit plugin add .`. Neither `add` nor `upgrade` builds the binary, so after an
upgrade bundle it again into the install path that `orbit plugin show research` prints:

```sh
cargo build --release -p orbit-research-cli --locked
scripts/bundle-plugin-binary.sh --binary target/release/orbit-research [PLUGIN_ROOT]
```

Regenerate request schemas with `scripts/generate-plugin-schemas.sh`.
[docs/plugin-validation.md](docs/plugin-validation.md) covers validation:
`tests/e2e_v1_loop.rs` runs in CI with a scripted agent, and `scripts/e2e-live.sh`
does a single real-crew run, gated by `--live` plus `ORBIT_RESEARCH_LIVE_CONFIRM=yes`.

## Terminal use

```sh
orbit-research --help
orbit-research workspace init ./my-corpus      # new corpus: Git repo on branch main
orbit-research research list --corpus ./my-corpus
orbit-research research show --corpus ./my-corpus --id Q001
orbit-research --format json research list --corpus ./my-corpus
```

Output is a table on a terminal and TSV in a pipe. Use `--format json` or
`ORBIT_RESEARCH_FORMAT=json` to get JSON. For an existing corpus, run
`workspace prepare-operations <dir>` once before linking work. It is idempotent
and never touches records or history. `research plan` takes `--shape investigation`,
`contribution` or `synthesis`, and rejects flags that belong to another shape.

## Design docs

- [Architecture](ARCHITECTURE.md): crates, module ownership, plugin transport, panels.
- [Research contracts](docs/design/research-workbench/specs/contracts.md) and
  [workbench design](docs/design/research-workbench/1_overview.md). Both are
  partly superseded; see the note at the top of each.
- [Terminal design](docs/design/terminal-interface/2_design.md): output, errors, compatibility.
- The standalone dashboard design (`docs/design/user-interface/`) is superseded by the plugin panels.
