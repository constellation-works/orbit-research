# Native workflow: package 0.2, record schema v2, Orbit resource v1

Canonical scientific records live under `research/records` in an explicitly named owner
Git checkout. This package authors JSON; Orbit executes tasks. No SQLite projection,
scheduler, task database, browser service or automatic sibling migration is introduced.
The static export is ordinary JSON usable by a later browser with static serving.

## Fresh checkout acceptance workflow

Rust 1.89+ and Git are required. Use a new output directory outside the repository:

```sh
cargo test --workspace --locked
cargo build -p orbit-research-cli --bin orbit-research
ORBIT_RESEARCH_BINARY="$PWD/target/debug/orbit-research" python3 examples/native_workflow.py /tmp/research-native-demo
./target/debug/orbit-research validate /tmp/research-native-demo/physics-fixture/export.json
./target/debug/orbit-research validate /tmp/research-native-demo/parallax-fixture/export.json
```

The example runs every authoring operation through the installed CLI, retaining request
JSON, canonical appends, trace and validated export in two fresh temporary Git owners.
It creates a program/question, claim, exact apparatus and input pins, registered protocol,
run-start receipt, result artifact, completed/failed/cancelled runs, supported and
inconclusive assessments, then retires the claim while retaining both verdicts.
It also retries every append and reconciles an exported manifest. The physics-control
example concerns a model estimator; the Parallax R04/H01/E01 example concerns a synthetic
assistant evaluator. These are generated acceptance fixtures, not new scientific results
or experiments. Their explicit fixture Orbit links do not identify real tasks.

No sibling package, private dataset or live journal is needed. Keep generated
fixture checkouts outside the worktree. `examples/native_workflow.py` is a host
script that drives the Rust CLI; it does not import a Python package.

## Requests and exact revisions

All authoring commands share these explicit arguments:

```sh
orbit-research claim --owner-root /owner/checkout --repository owner --request /tmp/claim.json
```

A minimal claim request is:

```json
{
  "request_id": "claim-C1-original",
  "id": "C1",
  "expected_heads": [],
  "reason": "Original hypothesis capture.",
  "scope": "observation",
  "payload": {"role": "hypothesis", "statement": "Exact testable statement.", "domain": "empirical"},
  "orbit_links": [{"host": "hm_example", "workspace": "ws_owner", "task": "ORB-assigned", "run": "jrun-assigned"}]
}
```

Replace routing placeholders with the authoritative task envelope/discovery values.
`program` payload adds `role`, `title`, `question`; `artifact` uses the v1 artifact fields.
`preregister` accepts only `payload.semantic`; freeze times and receipt fields are generated.
`begin-run` and `record-run` use the complete v2 experiment payload except timestamps.
`assess` uses the assessment payload plus `evidence_summary`. The installed v2 schema and
the example's retained `requests/*.json` are executable payload references.

`heads --id <full URN>` returns the complete unsuperseded revision set. Every request
must provide the exact `expected_heads`, including `[]` for a new identity. Ordinarily
all these heads are superseded. Supply a `supersedes` subset explicitly to retain a
conflicting branch; no branch becomes unique truth automatically. Use a new request ID
for changed content. Reusing an identical key and request returns the original record,
even after later revisions. Reusing a key for different content fails.

Corrections append a new revision with exact predecessor references and a reason. The
old payload, semantic revision, assessments and bytes survive. `retire` accepts a program
or claim and its unchanged payload; retirement does not adjudicate or remove assessments.
Protocol revision IDs remain hashes of all normative `semantic` terms. A changed threshold,
comparator, control, full normative prose, input or claim pin produces a new revision.
Presentation alone cannot create a new protocol revision. Linked explanatory prose can be
maintained separately; never hide normative terms there.

Publish records through the owning repository's authorized Git procedure, then obtain a
pin with `ref --id <URN> --revision sha256:<digest> --source-revision <full commit>`.
The package never commits, merges, pushes or schedules work. `ref` verifies canonical content equality
against that commit and returns an exact resolved scientific reference. Author-time
provenance has `working_tree: true`; publication does not rewrite canonical files.
Exported views bind their actual source path, blob OID and bytes to the supplied Git commit.

## Registration chronology and scientific limits

Native freezes use `freeze: registered`, deliberately distinct from v1 `prospective`.
The CLI observes the current UTC time and binds the complete protocol to the immutable
append chain. It checks a full code commit in an explicitly routed checkout, exact
claim/input references, a holdout or seed-plan artifact digest, information cutoff and
an evaluation boundary at or after registration. It accepts no caller freeze timestamp.

A `begin-run` requires the exact native protocol already present in a Git commit ancestral
to the protocol owner's inspected HEAD. It observes start registration after the evaluation
boundary and binds the frozen code, inputs, holdout and operational links. Commit the start
receipt before registering results. This is a scientific record of execution intent; it
does not launch, monitor or schedule a process. Orbit owns actual execution.

`record-run` appends measured registration time, start reference when available,
execution status, per-control results, input/output artifact references, code, environment,
invocation and deviations. Runs without a native start receipt remain valid limited
history; they cannot support primary confirmation. Changed consumed inputs/code must have
explicit deviations and cannot retain primary confirmatory eligibility. Cancelled and
failed records do not imply scientific refutation. Historical imports cannot enter the
native append path or be promoted by supplying an old date.

This chronology proves local registration order and frozen input identity, not independent
attestation that data were untouched or computation never occurred elsewhere. The author
is responsible for honest holdout access and measured results. The library does not label
local chronology as independently verified prospective execution, fetch evidence from a
remote attestor, or retroactively promote a historical freeze. Filesystem access by a
malicious author and forged Git history are outside this cooperative authoring boundary.

Artifacts carry immutable digests and availability assertions supplied by their owner.
`available` requires a digest; this package does not download an external locator or prove
that claimed bytes were consumed. Large/private data stay outside Git under immutable
owner manifests. Unavailable artifacts and unresolved/wrong record pins block confirmation.

Primary confirmation requires a complete resolved evidence closure and an exact assessed
claim revision frozen in the protocol, completed execution, no deviations, native start
chronology, exact code/input/holdout agreement and every required control explicitly passed.
A caller's aggregate `controls: passed` cannot override failed or missing per-control
results. Mixed/unmeasured evidence cannot yield primary confirmation; supported/refuted
verdicts must agree with the declared evidence summary. Simulation/model/synthetic evidence
cannot establish primary confirmation of a claim about nature. Scope and domain stay
separate from task success, activity and verdict.

Empirical/synthetic protocols require enumerated positive sample counts matching their
total, a baseline, named controls, explicit metric/operator/attainable threshold range
and finite planned/maximum resource budgets. Optional exact one-sided binomial lower-bound
rules check attainability even with all successes; this catches the 44/47-style sample
accounting and impossible power gate class. Declared attainable ranges and prose analyses
are owner assertions, not automated power certification for arbitrary statistical methods.
Deterministic designs instead declare assumptions, convergence criterion/tolerance/step
budget and decision rule; no universal sample size or statistical test is required.

## Atomicity, publication and recovery

The writer serializes cooperating processes with a POSIX directory lock and rechecks heads
under that lock. Each fully serialized record is fsynced in a temporary file, published
with an exclusive atomic hard link, then the directory is fsynced. Neither files nor a
mutable index are overwritten. The file name includes sequence and a digest of the entire
record, including presentation and provenance; each record also names its predecessor
hash. Readers reject modified content, sequence gaps, duplicate request IDs and forks of
the append chain. Scientific branches live explicitly in `supersedes`, not as chain forks.

A failure before publication leaves no record. A failure after publication may have
committed the record even if the caller saw an error: retry the **identical** request.
An abrupt process death can leave a hidden `.append-*` staging file. It is not canonical;
after verifying that no writer is active, an operator may remove that staging file. Never
remove or rewrite a numbered JSON append. Local POSIX filesystems are the supported
atomicity boundary; network/object filesystems need their own verified publication layer.
These are owner records, not a second operational task store.

## Trace, export and cross-repository reconciliation

```sh
orbit-research trace --owner-root /owner/checkout --repository owner --id '<URN>' --revision 'sha256:<digest>'
orbit-research export --owner-root /owner/checkout --repository owner --source sibling=/sibling/checkout --source-revision '<full commit>' --output /tmp/export.json
orbit-research validate /tmp/export.json
orbit-research reconcile /tmp/manifest.json --target /tmp/exact-record.json --output /tmp/reconciled.json
```

Trace returns exact dependencies, code/data pins, Orbit links, all assessments of the
requested claim revision and explicit unresolved references. Export checks canonical
appends against the requested commit, reads dependency records from exact Git snapshots
in explicitly routed owners, and validates the full closure. It never fetches, selects
newer sibling revisions or writes to them. Alternate record directories must remain under
`research/`. Duplicate matching records in one snapshot are ambiguous and fail closed.

V1 manifests allow one source revision per repository. A v2 export therefore contains
multiple v1 manifests grouped by exact repository/source snapshot, plus their validated
record closure. This preserves links into several historical snapshots without upgrading
them to the latest commit. Unknown canonical IDs/revisions remain explicit owner missingness
outside typed references, as in the Principia pilot. `reconcile` returns a new manifest;
validation and reconciliation never mutate input documents or silently repair source pins.

## Supported operator installation and compatibility

`orbit-research resource --version 1` prints the packaged skill as JSON. Package installation
has no resource-registration or skill-install side effects. For Codex, an operator can copy
the installed resource to a new personal skill directory using the documented manual skill
layout (choose the intended user's skill root):

```sh
mkdir -p "$HOME/.codex/skills/orbit-research-native"
cp crates/orbit-research-cli/resources/v1/SKILL.md "$HOME/.codex/skills/orbit-research-native/SKILL.md"
```

Do this only as an operator on the intended host. There is no invented `orbit resource
install` command. Existing Orbit tasks refer to the versioned package resource or installed
skill through their ordinary instructions. `task-context` is a bounded, explicitly routed
client of `orbit tool run orbit.task.show`; it performs no workflow admission or writes.
The resource directs durable summaries through `orbit.task.update` on the same authority.
Tests exercise the real argv protocol with a temporary executable, reject mismatched
workspace/terminal responses, and create no live tasks or shadow stores.

Package 0.2 reads unchanged v1 schemas and historical records and writes explicit v2
native records. V1 semantic digests, aliases, compatibility views and historical meanings
are unchanged. Consumers that only understand v1 must opt into v2; do not rewrite old
records in place. Principia ORB-11378 deliberately pins package 0.1 at Git `7b6c1b2…` and
checks that VCS identity. Its owner must update that pin/checker in a separate task before
using native authoring; the existing 48 records and seven views remain valid v1 data.
Parallax ORB-11371's nested R/H/E documents now map through the shared adapter. E01's
exploratory status, E02's claimed preregistration, full methodology/results/limitations,
H08/H09 `revised`, and raw `archived` activity survive without stronger interpretation.
ResearchJournal uses `experiment_id`, separate identities from TradeJournal, and preserves
all intent/outcome columns. Temporary real-schema WAL fixtures demonstrate preservation;
no live ResearchJournal database was available and none is fabricated.

## Typed non-Git dataset verification seam

Astrolabe ORB-11380 (`1642b4ba…`) retains immutable Parquet/sidecar pairs outside Git.
The default v1 resolver still treats its working-tree/null-source records as pending.
Owners can now explicitly supply an `ArtifactResolver` to `validate`, `reconcile` and
`Owner { artifact_resolver, ... }` without changing those v1 records or inventing Git pins.
This is a library seam for the owner adapter; the generic CLI does not dynamically load
plugins or silently trust a serialized verification flag.

The resolver receives an exact reference and returns `None` or a `VerifiedArtifact`:

```rust
use orbit_research_owner::verify_artifact;

let proof = verify_artifact(
    &record,
    snapshot_directory,
    &record["legacy"]["snapshot"],
    &byte_fields, // parquet_sha256 -> dataset.parquet, sidecar_sha256 -> metadata.json
    owner_schema_and_sidecar_check,
    record_path,
)?;
```

The owner callback implements the typed `SchemaCheck` protocol, receiving explicit root,
record and descriptor and raising `ValueError` for mismatched parsed schema, row/column
facts, units, sidecar or parent meaning. Astrolabe already owns Arrow parsing and snapshot
verification; no Arrow/Astropy dependency or second importer is added here. An owning task
can wire this seam after updating its exact package pin.

The framework verifies the retained `record.json`, SHA-256 of every descriptor `*_sha256`
byte member (no omitted dataset/sidecar), the canonical descriptor digest against the
artifact snapshot digest, and exact `schema` and `parent_pins` bindings. It checks members
again after the owner parser. The capability rechecks on use, so missing/tampered bytes
or a changed record fail; it is not a durable attestation transferable to a machine without
those bytes. Paths stay beneath the explicit owner snapshot root and cannot use symlinks.

A verified artifact may resolve with its original `source_revision: null` or working-tree
provenance. Its source record, schema/sidecar/parent pins and missingness remain unchanged;
this does not convert current retained bytes into proof of historical consumption.
Unresolved or unrecorded lineage still blocks primary confirmation and no parent is
invented. Exact parent references must independently resolve. The historically absent Gaia
inputs and missing outputs remain missing. Export validation involving these capabilities
must supply the owner resolver again; the plain CLI correctly refuses to treat absent
external verification context as resolved evidence.
