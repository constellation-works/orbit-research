# Orbit plugin validation

The required local gate is `make test`. It covers the plugin request schemas,
exec input limits, launcher failures and a real Git-backed fixture corpus.
The plugin's sixteen conformance goldens (twelve tools) cover its corpus-independent
health response, deterministic refusals and the four panel tools' readable "no corpus"
output in Orbit's empty conformance workspace.

Before linking work in an existing corpus, prepare its operational journal
outside the plugin sandbox:

```sh
orbit-research workspace prepare-operations /absolute/path/to/corpus
```

New corpora created by `workspace init` prepare this directory automatically
on Linux and macOS. Preparation preserves existing idempotency entries and
the journal's lock while moving them to `_data/orbit-research-operations/`.
The plugin grants writes only to that ignored operational directory and its
`.orbit-research-tmp/` acceptance scratch directory. It does not need to write
Git metadata or canonical research records. An existing unprepared corpus
refuses linking with the preparation command before creating an Orbit task.

To exercise an installed plugin with an existing Orbit binary:

```sh
ORBIT_RESEARCH_TEST_ORBIT_BIN=/absolute/path/to/orbit \
  cargo test -p orbit-research-cli --test plugin_v2 --locked -- --ignored
```

This test copies the canonical plugin into a temporary directory, bundles the
current executable and creates its own corpus, HOME and Orbit workspace. It
invokes all twelve advertised tools through both Orbit CLI and Orbit MCP:
`version`, `list`, `show`, `check`, `plan`, `link`, `validate`, `accept` and the four
panel sources `open-questions`, `awaiting-acceptance`, `hypotheses` and `corpus-health`.
The panel sources answer their readable empty state for the fixture's one reserved
result.
It checks real task creation and idempotent linking, including an omitted task
description, and malformed `version` fields. It first models an unprepared
legacy journal and requires both transports to refuse linking before any task
is created, with the explicit preparation command. Preparation preserves
existing receipt and lock bytes; subsequent linking and retries leave the
scientific commit unchanged and Git status clean. `validate` must refuse without
job context; `accept` must refuse a task that has not delivered. CLI calls use
Orbit's explicit operator override and MCP uses its operator session.

Positive delivery validation, positive acceptance, evidence mismatch,
idempotent acceptance and failure cleanup run through the same exec handler
against a Git-backed fixture with an in-memory task host:

```sh
cargo test -p orbit-research-cli --bin orbit-research --locked tests::plugin
```

The same fixture feeds `assess`: the lookup that production composes reads what
`accept` stored, a README amended afterwards is refused as stale, and a fake
`orbit` executable (named by `ORBIT_BIN`) covers the process adapter and its
refusals, including through the real binary's CLI and MCP surfaces
(`tests/assess_acceptance.rs`).

The panels' rows are pinned the same way, over a fixture with open and answered
questions, a hypothesis whose two results disagree, delivered and reserved
results, an empty corpus, no corpus and an invalid corpus:

```sh
cargo test -p orbit-research-cli --bin orbit-research --locked tests::panels
```

A second installed test, `awaiting_acceptance_reads_task_artifacts_through_callbacks`,
delivers the fixture result and calls `awaiting-acceptance` through the operator CLI
and through an MCP session without operator capability (the capability a dashboard
panel read uses). It requires the task to read `awaiting acceptance`, which proves the
`orbit.task.show` and `orbit.task.artifact.get` callbacks answer a read-only panel
source rather than leaving it `acceptance unknown`.

These deterministic checks do not run an investigation provider. The installed
test changes no existing Orbit workspace or plugin installation. It is ignored
in the ordinary test suite because it needs Orbit's native sandbox; CI runs it
with a checksum-verified Orbit 0.25.0 binary, alongside validation and conformance
testing of a clean Git export.

`tests/assess_installed_orbit.rs` runs `research assess` against a real Orbit
binary in a private HOME and Orbit root, without installing or enabling any plugin:
it puts the `accept`-shaped artifact on a real task with `orbit.task.artifact.put`
and checks acceptance, a missing artifact, an amended README and an unreachable
Orbit. It is ignored in the ordinary suite and runs in the same CI job:

```sh
ORBIT_RESEARCH_TEST_ORBIT_BIN=/absolute/path/to/orbit \
  cargo test -p orbit-research-cli --test assess_installed_orbit --locked -- --ignored
```

## The v1 loop, end to end

`tests/e2e_v1_loop.rs` is the repeatable proof that the whole v1 loop works on a
disposable corpus. It is ignored in the ordinary suite and runs in the same CI job as the
other installed tests, with `--nocapture` so each scenario's evidence is readable in the log:

```sh
ORBIT_RESEARCH_TEST_ORBIT_BIN=/absolute/path/to/orbit \
  cargo test -p orbit-research-cli --test e2e_v1_loop --locked -- --ignored --nocapture
```

It creates the corpus with `workspace init` (the bundled schema) in a private HOME and Orbit
root, installs and enables the canonical plugin there (the helpers are shared with
`plugin_v2.rs` through `tests/common/`), and drives the ten scenarios of the plugin spec's
"Acceptance scenarios for v1" through the real installed plugin and the real
`orbit run job research_investigation`. Every assertion and log line is prefixed with its
scenario name:

| Scenario | What the test does |
|---|---|
| capture-and-return | `capture` a question, `list` it from fresh processes and the plugin, no task exists, `open-questions` shows it |
| reserve-and-link | reserve two hypotheses and seven results, `plan` then `link` each: one task per result, an identical retry adopts it |
| investigation | `orbit run job` with a scripted agent: every step completes, the task reaches `review`, the delivery commit holds exactly the README, manifest, code and artifact |
| validation gate | four runs whose record has a missing section, a wrong run id, a tampered input digest or a hand-allocated id fail at the `plugin.tool_call` validate step; nothing commits, the corpus stays clean, the task stays out of `review` |
| acceptance | `accept` refuses a failed run and a never-dispatched task and stores nothing; before acceptance `assess` refuses; `accept` stores `research-acceptance.json` (read back through `orbit.task.artifact.get`) and a re-run is idempotent |
| negative result | a run whose control fails delivers a valid result, is accepted, and `assess` records `inconclusive` |
| revised hypothesis | a body edit bumps the revision to 2 and reopens the status while the revision-1 assessment stays; `assess` against revision 3 refuses |
| concurrency | four simultaneous primary writers get four ids and four commits; a stale expected blob refuses |
| sandbox | the manifest has the default sandbox, no `unsandboxed` and no `requires.programs`; `orbit plugin doctor` has nothing to report for `research` |
| corpus independence | only owner-layout paths are tracked, no file names Nebula or a sibling path, no remote, every worktree lives in the disposable root, the schema is the bundled one |

The four panels are read before acceptance (both delivered results `awaiting acceptance`),
after `accept` (the accepted one is gone), after the assessments and the revision (the
hypothesis rows, current and superseded) and finally through an MCP session without operator
capability, which must equal the CLI answers.

The investigation agent is scripted, not a provider. Orbit's executor definitions are YAML
files in the Orbit root (`resources/executors/<name>.yaml`); Orbit starts the definition's
`command` with the prompt on stdin, the run worktree as working directory and the run's
`ORBIT_*` environment, and reads the terminal `agent_message` of its stdout as the response
envelope. Orbit's own fake-agent tests substitute a CLI exactly this way
(`crates/orbit-core/tests/pi_fake_agent.rs`, `orbit-agent/src/providers/codex/codex_output.rs`
for the Codex JSONL shape, and the executor-onboarding runbook). The test points the shipped
`codex` executor at `tests/fixtures/e2e_agent.sh`, makes the `sol` crew the default and enables
it, so `worktree_setup`, the shipped `research_investigate` `agent_loop` step, the plugin's own
validate step, `git_commit`, `git_merge` and `update_task` all run unmodified. The script does
what the activity's instruction tells an agent to do (reads the stub, runs a small experiment
with a control, writes the README and manifest with the worktree-mode writer, names the new
files as task context and persists an execution summary) and never commits or moves the task.
Nothing calls a provider. The executor gets `allow_fallback: true`, because GitHub's Ubuntu
image has no `/usr/bin/bwrap` and Orbit otherwise refuses to start the agent; the fallback
applies only where the trusted sandbox binary is missing.

One difference from production: a directory install has no verified first-party origin (Orbit's
`first_party_source` accepts only a `git+` URL of a constellation-works repository), so its tools
register as `research.<verb>`. The test's export rewrites the `research_validate` activity from
`orbit.research.validate` to `research.validate`; no other plugin file differs from the
repository.

## The live smoke run

`scripts/e2e-live.sh` is the operator script for the one live run: the same loop with a real
crew, on a disposable corpus, after the plugin has been installed and enabled on the host. It
spends real money, so it does nothing but print its plan unless it is given `--live` and
`ORBIT_RESEARCH_LIVE_CONFIRM=yes`. The guard exists because an earlier version of the script
was run for real while its argument handling was being tested; its refusal paths are now covered by
`tests/e2e_live_script.rs`, which runs the script with a stub `orbit`, a private HOME and a PATH
holding only the stub plus the system directories.

```sh
scripts/e2e-live.sh --plan                       # steps only, touches nothing
ORBIT_RESEARCH_LIVE_CONFIRM=yes scripts/e2e-live.sh \
  --live --corpus /path/outside/any/repo/live-smoke --crew <crew>
```

The corpus directory must be new (or empty) and outside every Git work tree. The script
registers it as an Orbit workspace, captures a small deterministic question (a seeded fair coin
against a 60%-heads control coin over 100,000 flips), creates the hypothesis and the reserved
result, drafts and links the task, runs `orbit run job research_investigation`, waits (at most
`--max-minutes`, default 45), reports the delivered Result section and the provider and model
Orbit recorded, runs `accept` twice (the second must be idempotent), appends the assessment you
name with `--verdict`/`--strength` (default `inconclusive`/`anecdote`; it never infers one) and
prints the four panels. It never installs, enables, upgrades or grants anything. Evidence goes to
`<corpus>.evidence/`: `commands.log`, `timings.tsv`, every command's stdout and stderr,
`run-show.json`, `summary.txt`, `panels/`, `artifacts/`, `state.env` (ids, so `--resume` skips
finished steps) and `cleanup.txt`, whose commands (workspace deregistration, worktree removal,
deleting the corpus) are printed at the end and never run.

The manifest requires Orbit 0.25.0 or newer. On macOS, Orbit 0.24.0 denied
metadata access to its callback-session directory, preventing the callback
resolver from recognizing the host's live identity even though the inherited
descriptor remained open. The host fixed this in
[the ancestor metadata grant change](https://github.com/constellation-works/orbit/commit/73beabbc1bfc6091ffdcbea719d234a8a062b825).
The old host passed all eight empty-workspace conformance goldens it then had but failed a
real installed `link` call. Installed callback checks therefore remain a
separate required CI gate. The shared Linux/macOS manifest range keeps the
plugin inactive on Orbit 0.24.0: validation exits successfully with a host
incompatibility warning, installation stages a disabled plugin, and enablement
records grants but reports the plugin as inactive. Tool calls then fail with
`policy_denied` and the required host version before the backend runs.
