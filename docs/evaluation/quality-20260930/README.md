# Research CLI, MCP, persistence, and plugin validation

This audit starts from `1ca7a26` and covers the actual research executable,
Core/Store APIs, and the canonical `.orbit-plugin/` export on macOS arm64.
Private corpora and Orbit roots isolate the fixtures from installed user plugins
and live research data. Release versions, tags, and publication are outside this
change.

## Executable surfaces

The CLI has 15 leaf commands: `workspace init`, `workspace prepare-operations`,
`research list`, `research show`,
`research check`, `research create`, `research capture`, `research revise`,
`research assess`, `research revise-question`, `research work-links`,
`research plan`, `mcp`, `orbit-tool`, and `resource`. Real executable tests cover
every help page, both version flags, initialization, all three read-only plan
shapes, writing and retry behavior, missing/ignored plan arguments, four output
formats, actionable corpus failures, empty results, and closed stdout.

`tests/mcp_workflow.rs` checks the exact ten advertised tool names, executes them
through the actual MCP subprocess, and verifies idempotent create/revise retries,
stale-edit refusal, and unchanged HEAD/status after refused operations.
`research.assess` is exercised as a refusal through both CLI and MCP: production
composition deliberately uses `NoAcceptanceStore`, so positive assessment still
requires an injected acceptance adapter. Existing Core acceptance tests cover
that policy boundary; this audit does not introduce another receipt store.

Manual native Mac PTY checks covered a narrow 40-column table, Unicode text,
column-drop notice, complete detail output under `TERM=dumb`, and pretty JSON.
Human output escapes terminal controls; JSON/NDJSON retain structured values.

## Regression and boundary checks

Focused tests reproduced the original failures before fixing malformed MCP
envelopes and response IDs, silently discarded plan flags, competing link
confirmations, corpus schema symlinks, plugin null/object/schema drift, unsafe
launcher JSON, empty work-link diagnostics, protocol broken pipes, and shared
acceptance staging files.

MCP responds to ping before initialization without granting tool access or
advancing the handshake. A live process with stdin open returned its empty ping
result in 6 ms; malformed ping parameters remained invalid and tools remained
gated. MCP distinguishes protocol errors from tool execution failures. Invalid caller
arguments receive `-32602`, internal dispatch failures receive `-32603`, and
execution failures remain tool results with `isError`. Notifications cannot
mutate the corpus. A closed stdout pipe exits quietly only when the protocol
writer observes that actual write failure; input failures and other I/O errors
still fail.

Corpus schema files and ancestors must be ordinary paths on open and on later
reads. Link confirmation holds the request-log lock across inspection and
save; a competing confirmation cannot replace its original task correlation.
Acceptance calls use unique owner-only staged files, preserve unrelated files,
refuse a symlinked scratch directory, and clean up after callback success or
failure. Overlapping calls retain independent callback sources.

The installed Linux test exposed a separate write denial: its sandbox correctly
forbids creating the old request log inside `.git`. Operational state now lives
in the primary checkout's ignored `_data/orbit-research-operations/`, with that
directory as the plugin's additional write grant. Fresh Linux/macOS initialization
prepares the layout; existing corpora require the explicit preparation command.
Plugin linking refuses before persisting an intent or invoking a task callback
when the corpus has not been prepared.

Preparation holds the original lock and atomically exchanges the complete log
with a versioned marker. Tests preserve pending and confirmed receipts byte for
byte, keep the original lock inode, prove contention across linked worktrees,
and exercise a reader waiting on the old descriptor during publication.
An unsupported exchange preserves the original log; retries recover a staged
marker. Unknown layouts, populated targets, tracked paths, unignored entries,
symlinks and special files refuse without adopting foreign state. Older clients
refuse the marker instead of opening a second journal. CLI checks verify
unchanged canonical records, schema, ignore policy, HEAD and index, including
repeated preparation in all four output formats.

Git subprocesses and in-process readers respect the explicit corpus rather
than inherited repository, work-tree, index, object or inline-config overrides.
A private baseline export at `cd11161` reproduced a primary writer consulting
the foreign dirty index. The candidate regression exercises three independent
override modes, owner ignore/index checks during preparation, committed reads,
reservation and fresh initialization, while preserving the complete foreign
repository byte for byte. The same test caught the in-process work-tree override
before its permission fix. This does not claim that every inline-config mode
redirects Git's `status` command on every platform.

The pristine required `make test` gate passed all 176 tests. The final `make ci`
gate passed with the storage and Git-isolation changes: 223 tests passed and the installed
Orbit test was ignored by the ordinary suite. Formatting, the required CLI
all-target Clippy check, dependency-direction self-tests, and whitespace checks
passed. The locked workspace build and documentation with warnings denied also
passed. Extra production Core/Store library Clippy checks passed; historical
test-fixture unwrap warnings remain outside this scoped cleanup and are not
suppressed or turned into a new required gate.

## Installed Orbit plugin

The [installed test](../../../crates/orbit-research-cli/tests/plugin_v2.rs)
creates a private HOME, corpus and Orbit root, installs the canonical plugin
with the real research binary, and invokes all eight advertised tools over
both Orbit CLI and MCP: `version`, `list`, `show`, `check`, `plan`, `link`,
`validate`, and `accept`.

Read tools return actual corpus evidence. Link creates a real task and adopts
the exact same task on retry, including when its optional description is
omitted. The title supplies a valid description before intent persistence;
blank titles fail without writing an unresolved intent. `validate` refuses
missing job context and `accept` refuses a task that has not delivered.
Positive delivery validation and acceptance, evidence mismatch, retries and
cleanup are covered by Git-backed fixtures using the same exec handler and
an in-memory task host. These checks do not run an investigation provider.

The installed test passed against Orbit 0.25.0, including the prepared-state
workflow and preservation of an actual pending legacy receipt. The rebuilt
candidate's native test completed in 22.77 seconds. Clean exports at `dbff799` passed
all eight conformance goldens on both Orbit 0.24.0 and 0.25.0, but the installed
test exposed a macOS callback failure on 0.24. A separate shell-only backend
using the same selected 0.24 executable reproduced the refusal, excluding the
research binary and its Rust subprocess handling. Metadata-only probes showed
the authority descriptor stayed open and ordinary with close-on-exec cleared;
its contents were not inspected.

On 0.24, both `stat` and `realpath` of the private session parent failed with
`EPERM`. The same 0.25 probe succeeded, and its task callback succeeded. Neither
probe listed the directory or read the identity record.

The tagged 0.24 callback verifier canonicalizes the session parent directory,
whose metadata is denied by that release's macOS sandbox. Orbit 0.25 includes
the [upstream metadata-ancestor fix](https://github.com/constellation-works/orbit/commit/73beabbc1)
without granting directory contents or listing. The plugin therefore requires
Orbit 0.25 for its combined Linux/macOS contract; it retains strict callback
identity and its plugin release version. On 0.24, validation warns and installation
stages a disabled plugin; enablement records grants but leaves it inactive. Both
version and link calls then fail with the required-version diagnostic before
the backend runs. This does not assert that every Linux 0.24 callback is broken.
The corrected export passed all eight conformance goldens, and the installed
test passed with a checksum-verified official Mac 0.25.0 release binary.
The separate CI plugin job pins a checksum-verified
supported Orbit binary and runs clean-export conformance and the installed test.
See [plugin-validation.md](../../plugin-validation.md) for commands and coverage
boundaries.

## Reproducible committed-snapshot benchmark

The [locked harness](benchmarks/committed/run.py) builds a copied source tree
outside the checkout and creates a private committed corpus with 999 questions.
It calls the public `Corpus::snapshot` and `Corpus::committed_snapshot` APIs in
release mode. Every result checks the revision, record count, IDs, complete
bodies, Git blob IDs, and content digests against the expected corpus.

Each process performs one warmup and 21 measured calls per API, reports the
median in milliseconds, and retains every sample. Three independent process
repeats use the same deterministic fixture. Source fingerprints and the locked
harness distinguish the exact builds being compared.

The optimization in `899b99d` keeps one repository and pinned commit tree alive
for each committed snapshot, rather than reopening Git for every record. There
is no cache across calls. Tests verify that an in-flight view keeps its original
schema, records, and paths as HEAD advances, while the next call on the same
corpus handle observes the new commit. Ordinary-file checks still reject
committed symlink records.

| Public API | Baseline repeat medians (ms) | Optimized repeat medians (ms) | Median of repeats, before → after |
| --- | --- | --- | --- |
| `committed_snapshot` | 502.043, 483.108, 494.536 | 90.746, 90.849, 115.423 | 494.536 → 90.849 ms |
| `snapshot` | 55.311, 35.344, 37.428 | 35.300, 37.340, 36.662 | 37.428 → 36.662 ms |

The committed-read summary improves about 5.44×. Every committed repeat improved
substantially; working-read variation reflects host load and is not a separate
performance claim. Full distributions, fixture commit, Rust 1.95.0 and gix
0.88.0 metadata are retained in [results.json](benchmarks/committed/results.json).

```sh
python3 docs/evaluation/quality-20260930/benchmarks/committed/run.py /path/to/baseline --offline
python3 docs/evaluation/quality-20260930/benchmarks/committed/run.py /path/to/current --offline
```

Omit `--offline` if dependencies are not cached. The script retains and prints
its temporary project directory. It strips inherited `GIT_*` overrides before
subprocesses and supplies private fixture identities/configuration. A hostile
`GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`/inline-config check verified that the
fixture created its own repository and left a separate temporary repository's
HEAD, index, and working tree unchanged. These synthetic corpus measurements describe
snapshot reads, not end-to-end research-job throughput.
