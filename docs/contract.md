# Scientific record contract, version 1

Owning scientific repositories retain canonical records. This package validates JSON and
proposes historical imports. Reports are candidates for owner reconciliation, not an
alternative authority. No task completion, successful process or delivery changes a
scientific verdict. No scheduling, database projection, service or native authoring is
implemented in the v1 foundation. Package 0.2 adds explicit v2 native authoring;
see [Native workflow](native-workflow.md). The v1 historical schema and digest rules below
remain compatible.

## Records and independent dimensions

Every record has `schema_version`, `kind`, stable `id`, content `revision_id`, aliases,
provenance, activity, scope, limitations, missingness, complete legacy data, references,
presentation and a kind-specific payload. Unknown fields on the canonical envelope/payload
are rejected; arbitrary legacy fields remain under `legacy` and in report `inventory.raw`.
The distributed schemas use JSON Schema Draft 2020-12. `record.schema.json` is the full
contract; six kind entry points reference it by its versioned URN. Package validators
register all schemas locally and never fetch schema URLs.

| Kind | Meaning | Deliberately separate |
| --- | --- | --- |
| program | Empirical program or theory, with explicit role | Program activity is not a verdict |
| claim | Exact statement; hypothesis is a role; model/nature/empirical domain | Claim revisions do not overwrite assessments |
| protocol | Immutable semantic content and its digest | Presentation and retrospective descriptions |
| experiment | Planned/running/completed/failed/cancelled/unknown execution, protocol and result references | An outcome is not scientific support |
| artifact | Source, external dataset or result with availability and snapshot digest | Large bytes stay with their owner |
| assessment | Claim revision, verdict, inference type, controls, evidence and rationale | Execution and research retirement |

`activity`: active, paused, retired, resolved, unknown. Retirement retains all revisions,
assessments, aliases and source records. The importer never deletes anything.

`scope`: derivation, simulation-under-assumptions, synthetic-calibration, observation,
literature, unknown. A supported model property does not become a claim about nature.
An empirical program need not invent a physics theory or deterministic-proof power rule.
Unknown scope is explicit missingness; the exact legacy qualification remains available.

`verdict`: supported, refuted, inconclusive, conditional, untested, unknown. Historical
`mixed` maps to inconclusive, `conjecture` to untested; other unfamiliar terms stay unknown.
The original term remains `legacy_verdict` and raw source status. The semantic validator
rejects changed/strengthened historical verdicts and support/refutation based on execution
alone. Historical claims retain reported support as historical evidence, not independently
verified current confirmation. Controls cannot be inferred from `control_ran: true`.

Confirmatory primary inference requires a known scope, passing or explicitly inapplicable
controls, scientific evidence and exactly resolved targets. A referenced experiment with
failed controls also rejects primary confirmation even if the assessment falsely says
its controls passed. Model/synthetic scopes cannot confirm a claim about nature. Validation
checks declared records; it cannot certify that an author's measurements or statements are
true. Historical import always uses `inference: historical` and `basis: legacy-report`.

## Identity, revisions and provenance

An owner supplies a stable namespace (for example `principia`, `parallax`). IDs have the
form `urn:research:<repository>:<kind>:<percent-encoded-legacy-id>`. The same legacy ID in
two repositories or record kinds is distinct. Literal `%`, `:`, Unicode and `/` are encoded
without slugification. Dataset identity includes the explicit source dataset kind and name.
A filesystem path is a locator, never a scientific identity. Anonymous source artifacts
use an immutable content/selector identity. Repeated identities in an import are reported
as collisions, with all raw records retained; no winner is silently selected.

Non-protocol revision IDs are SHA-256 over canonical JSON of the record excluding
`revision_id`, `presentation`, and `provenance`. Canonicalization is this version's Python
JSON encoding: sorted keys, UTF-8, no ASCII escaping, compact separators, finite numbers.
It is **not** advertised as RFC 8785/JCS; number representations and Unicode normalization
are not rewritten. Source byte SHA-256 is separate from semantic JSON identity.

Provenance records the explicit repository namespace, full inspected Git HEAD (or null),
Git blob OID at HEAD (or null), SHA-256 of actual input bytes, relative source path, exact
record selector, historical origin and whether bytes differ from the pinned blob. Dirty
and ignored inputs are not misrepresented as the HEAD blob. Null Git pins are explicit
missingness for non-Git source snapshots. `--expect-revision` fails closed on a mismatch.
Reports also include immutable digests for encountered dataset bytes and SQLite sidecars.
Changing source HEAD, any inspected bytes, or the SQLite sidecar set during import fails
rather than claiming an unchanged snapshot. This is a consistency check over the read
interval, not a lock on another process; owners should supply quiescent snapshots.

`orbit_links` may carry `host`, `workspace`, `task`, and optional `run` identifiers solely
for operational traceability. They confer no scientific authority. Importers never read
Orbit's internal stores and do not resolve bare legacy `ORB-*` mentions into invented host
or workspace identities. Such source mentions remain raw source data.

## Protocol semantics and freeze

`protocol.payload.semantic` contains **all normative terms**: question/hypothesis, scope
and assumptions, source/input pins, design and interventions, comparator, controls, outcome
measures, exclusions, stopping rules, analysis, decision thresholds and any domain-specific
power or sensitivity requirement. Owners must explicitly identify which terms apply.
`semantic_digest` and protocol `revision_id` are SHA-256 of this object only.

Moving a source file, editing explanatory `presentation`, or updating provenance after
unrelated prose edits leaves a frozen semantic revision unchanged. Changing a comparator,
threshold, exclusion, seed policy, control or other normative term changes the revision.
Callers append that new revision; they do not overwrite old frozen content. Validation
rejects a changed semantic object presented under the old digest. Normative terms must
never be hidden in presentation. The validator checks content addressing, not arbitrary
natural-language equivalence or a remote repository's append-only enforcement.

Gate imports preserve their entire historical object under `semantic` and label it
`historical-unverified`. Markdown remains a source artifact. Neither text saying
“preregistered” nor a journal timestamp proves a prospective freeze. Historical records
cannot carry prospective freeze assertions. Owner pilots must recover the original pinned
normative object and independently establish chronology before publishing an authored
protocol. Later explanatory prose in a source document is not that original frozen object.

## Cross-repository manifest and reconciliation

`manifest.schema.json` pins repository IDs and exact Git revisions, and references stable
record IDs plus semantic revision IDs and source revisions. Reports start every reference
`pending`. Legacy links and lineage names/timestamps remain raw with explicit exceptions;
the importer does not fetch or silently select today's target.

`reconcile(manifest, records)` returns a copy. It resolves only a unique, valid supplied
record with the exact repository/ID/revision/source pin and clean pinned source bytes.
Package 0.2 additionally accepts an explicit typed `artifact_resolver` for independently
rechecked non-Git dataset bytes; see the owner-verifier seam in
[Native workflow](native-workflow.md). The default remains conservative and unchanged.
Missing, dirty, duplicate and wrong-revision targets remain pending. It does not mutate
records or contact a repository. `validate(manifest, targets=records)` checks a reconciled
manifest; the CLI accepts repeated `--target` files for the same purpose. A pending link
is valid missingness, but is never current confirmatory evidence. Unversioned external
artifacts need an owner-published immutable manifest before promotion; a present-day digest
cannot retroactively establish what was consumed by an old run.

## Projection/browser and workflow seam

A later index may ingest validated owner manifests keyed by `(id, revision_id)` with
provenance and separate reference-resolution state. Its SQLite tables are disposable
projections, rebuilt from owner records; they may not become a second task or scientific
store. A local browser can consume exported JSON through static serving. This milestone
adds no service, scheduler, speculative plugin API or sibling cutover. Package 0.2 now supplies native authoring and preregister/record-run/assess commands
through explicit schema v2. Existing owner pilots keep v1; cutovers require owner tasks.
