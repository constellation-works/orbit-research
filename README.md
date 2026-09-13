# orbit-research

Scientific claims, protocols, evidence and assessments, with reproducible workflows powered by Orbit.

This Rust CLI and crate workspace provides immutable native scientific authoring and
conservative read-only legacy imports, a disposable SQLite index and a local static evidence
browser. Principia, Parallax, Orrery and Astrolabe retain their
own records and artifacts. Orbit remains authoritative for operational tasks, crews, runs
and delivery. The approved starting contract is Constellation task ORB-11353; native
authoring is ORB-11391 (native record schema v2); the index/browser is ORB-11392. The
Rust crate graph that now owns those seams is ORB-12384–12389.

## Build and validate

Requires Rust 1.89+ and Git for source-revision pinning.

```bash
cargo test --workspace --locked
cargo run -p orbit-research-cli --bin orbit-research -- validate examples/migration-report.json
```

`make test` is the complete required gate (fmt, clippy, crate tests, crate-graph check).
No scientific owning package, source datasets or database service is required.

## Reproduce the four portable dry runs

The fixture generator creates synthetic source trees and a small journal SQLite database.
Choose fresh paths: both generator destination and report output files must not exist.

```bash
python3 examples/make_fixture_sources.py /tmp/research-fixtures
cargo run -p orbit-research-cli --bin orbit-research -- import principia --source-root /tmp/research-fixtures/principia --repository principia --dry-run --output /tmp/principia-report.json
cargo run -p orbit-research-cli --bin orbit-research -- import parallax --source-root /tmp/research-fixtures/parallax --repository parallax --dry-run --output /tmp/parallax-report.json
cargo run -p orbit-research-cli --bin orbit-research -- import orrery --source-root /tmp/research-fixtures/orrery --repository orrery --dry-run --output /tmp/orrery-report.json
cargo run -p orbit-research-cli --bin orbit-research -- import astrolabe --source-root /tmp/research-fixtures/astrolabe --repository astrolabe --dry-run --output /tmp/astrolabe-report.json
cargo run -p orbit-research-cli --bin orbit-research -- validate /tmp/principia-report.json
cargo run -p orbit-research-cli --bin orbit-research -- validate /tmp/parallax-report.json
cargo run -p orbit-research-cli --bin orbit-research -- validate /tmp/orrery-report.json
cargo run -p orbit-research-cli --bin orbit-research -- validate /tmp/astrolabe-report.json
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
cargo run -p orbit-research-cli --bin orbit-research -- import parallax --source-root /home/daniel/workspace/constellation/codebases/parallax --repository parallax --expect-revision 284ae537ae977d149364094c14cc5e1621b5b03e --dry-run --output /tmp/parallax-live-report.json
```

## Contract and evidence

- [Rust port design](docs/design/rust-port/1_overview.md): crate graph, Python-compatible
  digests, and the drop-in CLI gate.
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
- [Version 1 schemas](schemas/v1/record.schema.json): embedded by `orbit-research-contract`.

CLI entry points include `validate`, `reconcile`, `import`, owner operations (`program`,
`claim`, `artifact`, `preregister`, `begin-run`, `record-run`, `assess`, `retire`, `heads`,
`ref`, `trace`, `export`), and `index` / `index-trace` / `browse-export`. `validate` returns
errors rather than modifying data; `reconcile` returns a copy with exact supplied pins
resolved. No import command writes scientific records. The owner commands provide native
appends, exact reference resolution, trace and validated export. The index is a disposable
projection; it never writes scientific records. Sibling cutovers remain owning-repository work.
