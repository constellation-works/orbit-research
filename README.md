# orbit-research

Scientific claims, protocols, evidence and assessments, with reproducible workflows powered by Orbit.

This installable Python CLI/library provides immutable native scientific authoring and
conservative read-only legacy imports, a disposable SQLite index and a local static evidence
browser. Principia, Parallax, Orrery and Astrolabe retain their
own records and artifacts. Orbit remains authoritative for operational tasks, crews, runs
and delivery. The approved starting contract is Constellation task ORB-11353; this milestone
is ORB-11367; native authoring is ORB-11391 (native record schema v2); the index/browser is
ORB-11392 (package 0.3, unchanged scientific record schemas).

## Install and validate

Requires Python 3.11+ and Git for source-revision pinning. With `uv` installed:

```bash
uv venv /tmp/research-env
uv pip install --python /tmp/research-env/bin/python .
/tmp/research-env/bin/orbit-research validate examples/migration-report.json
/tmp/research-env/bin/python -m unittest discover -s tests -v
```

A standard virtual environment with `python -m pip install .` also works. On a runner with
a read-only default uv cache, set `UV_CACHE_DIR` to a writable directory outside the checkout.
No scientific owning package, source datasets or database service is required.

## Reproduce the four portable dry runs

The fixture generator creates synthetic source trees and a small journal SQLite database.
Choose fresh paths: both generator destination and report output files must not exist.

```bash
/tmp/research-env/bin/python examples/make_fixture_sources.py /tmp/research-fixtures
/tmp/research-env/bin/orbit-research import principia --source-root /tmp/research-fixtures/principia --repository principia --dry-run --output /tmp/principia-report.json
/tmp/research-env/bin/orbit-research import parallax --source-root /tmp/research-fixtures/parallax --repository parallax --dry-run --output /tmp/parallax-report.json
/tmp/research-env/bin/orbit-research import orrery --source-root /tmp/research-fixtures/orrery --repository orrery --dry-run --output /tmp/orrery-report.json
/tmp/research-env/bin/orbit-research import astrolabe --source-root /tmp/research-fixtures/astrolabe --repository astrolabe --dry-run --output /tmp/astrolabe-report.json
/tmp/research-env/bin/orbit-research validate /tmp/principia-report.json
/tmp/research-env/bin/orbit-research validate /tmp/parallax-report.json
/tmp/research-env/bin/orbit-research validate /tmp/orrery-report.json
/tmp/research-env/bin/orbit-research validate /tmp/astrolabe-report.json
```

`examples/migration-report.json` was generated from the synthetic Principia fixture. It
preserves a retired program, completed-execution prose, an inconclusive failed-control
assessment and an unverified historical protocol. Null Git pins explicitly identify the
non-Git fixture. A successful dry run can contain exceptions; inspect `inventory` and
`counts` before planning an owning-repository migration.

For live checkouts, supply the owning root and full expected Git revision. Repeat `--select`
for specific files, or use the documented discovery patterns. Reports go outside source
roots; full live reports retain private source content and should stay outside Git.

```bash
/tmp/research-env/bin/orbit-research import parallax --source-root /home/daniel/workspace/constellation/codebases/parallax --repository parallax --expect-revision 284ae537ae977d149364094c14cc5e1621b5b03e --dry-run --output /tmp/parallax-live-report.json
```

## Contract and evidence

- [Rust port design](docs/design/rust-port/1_overview.md): crate graph, Python-compatible
  digests, and the drop-in CLI gate. Python remains the running implementation
  until that gate is green.
- [Local evidence browser](docs/browser.md): explicit checkout/manifest mapping, atomic
  rebuild, portable export, exact reconciliation, safe media and rendered acceptance checks.
- [Native workflow](docs/native-workflow.md): fresh CLI examples, atomic appends,
  registration chronology, assessments, exact trace/export and versioned Orbit instructions.
- [Scientific contract](docs/contract.md): six record kinds, semantic protocol identity,
  independent activity/execution/verdict, exact provenance and pending reconciliation.
- [Import boundaries](docs/imports.md): discovery, preservation/exception accounting,
  SQLite snapshot safety, output guards and owner obligations.
- [Live inventory](docs/live-inventory.md): exact inspected Linux revisions, counts,
  validation results and next owner-specific migration work.
- [Version 1 schemas](src/orbit_research/schemas/v1/record.schema.json): shipped as package data.

Library entry points include `Owner`, `make_record`, `protocol_digest`, `validate`, `reconcile`
and `import_source`. `validate` returns errors rather than modifying data; `reconcile` returns
a copy with exact supplied pins resolved. No import command writes scientific records.
`Owner` provides native appends, exact reference resolution, trace and validated export.
`index`, `index-trace` and `browse-export` provide the disposable projection and static browser.
Sibling cutovers remain owning-repository work; the index never writes scientific records.
