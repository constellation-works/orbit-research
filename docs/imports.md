# Read-only migration contract

`orbit-research import ADAPTER --source-root ROOT --repository OWNER --dry-run` always
performs a dry run. Omitting `--dry-run` has the same behavior. It never invokes an owning
package, executes its scripts, opens its database in place, or writes a scientific record.
Use `--select relative/file` repeatedly to replace the default discovery boundary with
explicit files. A Git root must be the checkout root; non-Git snapshot roots are accepted
with explicit missing Git provenance. Inputs under `.git` and `.orbit` are forbidden.

Reports go to stdout or a **new** `--output` file outside the source root. Existing files,
hardlinks, source-root outputs and source-escaping symlinks are refused. Parent directories
must already exist. Full reports retain exact source text, rows and fields and may contain
private content; do not commit live reports. The committed example is synthetic.

## Discovery and mapping boundaries

| Adapter | Default discovery | Mapping and explicit limits |
| --- | --- | --- |
| principia | `theory/**/claims.json`, `theory/**/*.md`, `gates/*.json`, `studies/*preregistration*.md`, `ledger.md` | Program/theory, exact claims, historical assessments, unverified gate protocols; prose/state/frontmatter/ledgers preserved as source artifacts |
| parallax | `docs/*.md`, `docs/research/**/*.md`, SQLite `.db/.sqlite/.sqlite3` under `data/` and `artifacts/` | Exact standalone H table rows and namespaced R/H/E frontmatter/Claim sections map conservatively; full prose and historical protocol/outcome qualifications retained |
| orrery | `lab/sims/**/*.json` | `sim.json` slug becomes a program/activity; result and run JSON remain immutable source/result artifacts pending owner mapping |
| astrolabe | `data/processed/**/*.json` | Explicit sidecar kind+name becomes dataset artifact; same-stem Parquet bytes are hashed by streaming; lineage remains pending |

Patterns deliberately exclude source code, environments, operational stores, templates and
bulk data scans. The report records its discovery patterns or explicit selections. Every
selected file gets a container inventory entry or a read/size/format exception. Every claim,
SQLite user-table row, recognized prose register declaration and nested JSON object array
member gets its own inventory entry. Scalar arrays/fields are retained in full in their
parent value; they are not counted as separate scientific records. Unknown arrays of
structured records are explicit `retained-member` exceptions. No parser drops unknown
fields. Unparseable/over-8-MiB metadata keeps a file digest and a whole-container exception,
not a false count of records it could not parse. Duplicate JSON keys and NaN/Infinity fail
parsing instead of silently overwriting/coercing values.

`counts.discovered = counts.mapped + counts.exceptions` counts inventory units, **not**
independent scientific claims, files or numerical samples. An exception may still carry
useful candidates when a dimension remains unresolved; `mapped` means no remaining mapping
exception for that inventory unit. `inventory.raw` preserves original values; source-file
SHA-256 separately identifies exact bytes. `aliases` enumerates every surviving candidate
alias. Collision candidates are withheld and their raw entries remain inventoried.

## SQLite specifics

The delivered Linux APIs retain separate `TradeJournal` and `ResearchJournal` schemas.
Trade outcomes use `trade_id`; research outcomes use `experiment_id`. Research intents
retain question, hypothesis, baseline, method, metric, invalidation and data cutoff in an
unverified historical protocol. `reject`, `revise` and `advance` remain raw operational
decisions, never inferred scientific verdicts. Distinct journal identity prefixes prevent
collisions across these APIs. Temporary real-schema WAL fixtures exercise this mapping;
no actual ResearchJournal bytes existed in the inspected Linux checkout. Other shapes are
fully inventoried with exceptions. Every user-table definition and row is read; BLOBs are
losslessly represented as `{"sqlite_blob_hex": "..."}`. A row containing a nonfinite SQL
number is excepted with an encoded value rather than producing invalid JSON.

The importer never constructs the owning journal class (its constructor creates schema
and directories). It streams database and WAL bytes into a private temporary directory,
checks source hashes before and after copying, and opens the copy with SQLite URI
`mode=ro` plus `query_only`. WAL reconstruction/SHM creation occurs only in the temporary
directory. It explicitly refuses rollback journals. Using `immutable=1` on a live source
would hide WAL-only rows and is deliberately avoided. The main DB, WAL, SHM and rollback
journal presence/bytes are checked again after import. Temporary snapshots are removed on
both success and error. Portable tests assert whole source trees remain byte-identical,
including a live WAL case with an open owning connection.

Intent hypothesis, rules and timestamps are retained without a prospective freeze.
Recorded outcomes become completed execution candidates with unknown controls and no
scientific assessment. Their textual result remains exact source data; profit/loss does
not become a scientific verdict. Absent journals are explicit missingness.

## Exit status and owner handoff

Exit 0 means a structurally/semantically valid dry-run report was produced; it can contain
many explicit migration exceptions. It does not mean migration or evidence validation is
complete. `validate` exits 1 for contract errors; I/O, source mismatch and invalid command
inputs exit 2 with JSON diagnostics. Use report counts/codes to plan owner reconciliation.

Principia owners must recover frozen protocol terms and resolve claim/control/scope and
ledger links. Parallax owners must identify actual owner journal exports and R/E semantics
on their host, preserve empirical scope and establish chronology without retrospective
preregistration. Orrery owners must map each result/run format to execution, protocol pins,
controls and separate assessments. Astrolabe owners must pin immutable dataset snapshots
and replace name/timestamp-only lineage with verified revision references while retaining
unpersisted parents as missing. None of these next actions is performed in sibling repos
by this framework implementation.
