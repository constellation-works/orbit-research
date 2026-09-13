---
name: orbit-research-native
description: Author and trace immutable scientific records with orbit-research 0.2 and native schema v2 in an owning repository, using an already assigned Orbit task for execution.
---

# Native scientific work, resource version 1

Canonical JSON and linked prose belong to the explicitly selected scientific owner.
Orbit owns tasks, runs, scheduling and delivery. This resource defines no job, routine,
store, service or automatic installation.

Read the assigned task and its newest comments through the authoritative Orbit connection.
Copy its discovered host/workspace selector and the injected task/run IDs; never infer
routing from cwd. Stop on a terminal task. Under a managed activity, retain the injected
checkout and leave start/review/delivery transitions to the pipeline.

Prefer granted Orbit MCP tools. On an authorized host with explicit CLI authority, the
ordinary tool path is:

```sh
orbit tool run orbit.task.show --root "$orbit_authority_root" --input-file "$task_request"
```

The request JSON contains `id`, `workspace` and `model: "codex"`. The absolute Orbit root
must be the operator-selected authority on the named host. For remote authority use the
connected MCP selector or an operator-provided remote CLI wrapper, never a local fallback.
`orbit-research task-context --orbit-root ... --host ... --workspace ... --task ... --run ...`
checks the returned task/workspace and emits an `orbit_link`. Its `--orbit-executable`
accepts that wrapper. A caller-supplied host label is routing metadata, not host attestation.
Unavailable authority or a mismatched response is a stop condition.

Use the installed `orbit-research` CLI with explicit `--owner-root` and `--repository`.
Every authoring command accepts `--request` JSON with stable `request_id`, owner-local
`id`, `expected_heads`, `reason`, `scope`, `payload`, and exact `orbit_links` including run.
Use `heads` to inspect all current revisions; never select a latest verdict automatically.
Reuse an identical request on retry. A stale base requires inspecting concurrent changes;
changed content requires a new request ID. Corrections append `supersedes`; they never
replace old files. Presentation changes belong in linked prose or separately identified
source artifacts; all normative terms belong inside protocol `semantic`.

Use `program`, `claim`, `artifact`, then `preregister`. Before `begin-run`, publish the
frozen record through the owner's approved Git path and obtain `ref` with full ID,
semantic revision and committed source revision. The protocol binds exact claim/code/input
pins and a holdout/seed-plan digest and future evaluation boundary. `begin-run` observes
registration time and requires the committed freeze. The caller subsequently executes the
approved experiment through Orbit, then `record-run` appends completed, failed or cancelled
execution with environment, invocation, outputs, control results and deviations. Scientific
support requires a separate `assess` request.

A freeze is `registered`, not independently attested prospective execution. Local ordering,
Git ancestry and declared holdout boundaries cannot prove that someone never inspected data
or ran code elsewhere. Never relabel historical imports as native records. Imported protocol
text and timestamps stay `historical-unverified`. Primary confirmation additionally checks
the complete exact evidence closure, frozen controls, no deviations, narrow claim/scope and
native start receipt. Failed/pending controls or mixed evidence require limited inference;
simulated evidence cannot validate nature. Deterministic studies use convergence and
assumptions rather than mandatory statistical tests.

Use `trace` with an exact revision, and `export --source-revision <full commit> --output
<new file>` for a validated static JSON bundle. Supply each sibling with explicit
`--source repository=/owner/checkout`; reads resolve exact Git objects, never current HEAD.
Pending locators are missingness, not authority. `reconcile` operates on supplied exact
records and leaves unresolved/wrong pins pending. Do not write sibling packages or pins.

For retained non-Git datasets, the owner may supply the typed `ArtifactResolver` library
seam. `verify_artifact` binds the complete descriptor, byte members, sidecar/schema and
parent pins and requires the owner's format/schema checker. Verification is rechecked on
use; a serialized flag is not proof. Preserve null Git provenance and unresolved lineage.
Do not promote missing historical bytes or install an owner adapter from this resource.

Persist a meaningful `execution_summary` via `orbit.task.update` on the same authority;
the CLI path is `orbit tool run orbit.task.update --root ... --input-file ...` with explicit
`id`, `workspace`, `model`, and `execution_summary`. Do not equate task delivery with a
scientific verdict. Follow the owner's validation and approval policy.

`docs/native-workflow.md` and `examples/native_workflow.py` provide executable
physics-control and nonphysics fixtures. They are acceptance fixtures, not experiments.
