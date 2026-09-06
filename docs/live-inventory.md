# Linux live import proof — ORB-11367

Inspected on 2026-09-06 from the actual Linux owning checkouts, read-only. The installed
`orbit-research` CLI performed a full default-discovery dry run and then `validate` for each
repository. All four commands exited 0, all reports passed validation, and the importer
verified unchanged hashes for every inspected source/associated dataset file. No sibling
scientific or operational records were written.

Exact source revisions:

| Repository | Full Git HEAD inspected |
| --- | --- |
| Principia | `13866b2848fbcce9a1f1ef2d4b97051a65f7e626` |
| Parallax | `284ae537ae977d149364094c14cc5e1621b5b03e` |
| Orrery | `f95ec6a6a5b0e0aceddcd67bbbf895c2858e3c0c` |
| Astrolabe | `90f5b58890da36c44286a4edbde7eead879410a8` |

[Machine-readable inventory](live-inventory.json) includes every inspected file's path,
HEAD blob OID, actual SHA-256 and working-tree flag, full-report digests, exact timestamp,
candidate-kind counts and exception-code counts. It contains metadata/digests only, no
source corpus or dataset bytes. Full reports remain in the execution's external temporary
output directory, not in this repository or a second scientific store.

| Source | Selected metadata files | All hashed files | Inventory units | Fully mapped | With exceptions | Candidates |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Principia | 90 | 90 | 211 | 0 | 211 | 332 |
| Parallax | 6 | 6 | 19 | 7 | 12 | 13 |
| Orrery | 81 | 81 | 16,608 | 0 | 16,608 | 73 |
| Astrolabe | 27 | 54 | 50 | 16 | 34 | 27 |

Inventory units include nested object-array records and explicit discovery gaps, not
independent claims. Candidates can coexist with exceptions. Scalar arrays remain intact
in their containing raw record. These counts describe the documented selection boundary,
not every file or every scientific assertion in the owning repositories.

## Findings and next owner obligations

**Principia:** 10 theory/claim containers yielded 10 programs, 121 exact claims and 121
historical assessments; four gates yielded unverified historical protocol candidates and
76 prose files yielded immutable source artifacts. All need at least one owner decision:
121 evidence/control/scope reconciliations, 10 legacy program-state mappings, four verified
normative freezes and 76 prose/ledger/state mappings. Wide-binary control claims remain
`mixed`/inconclusive; the independently supported oracle claim stays historical support
for that model property. No supported claim about nature was inferred. The source itself
pins an older frozen protocol while present-day explanatory prose differs: the owner must
recover the original normative terms rather than hash today's whole prose as its freeze.

**Parallax:** this is the Linux `284ae537` checkout whose registered origin is the private
bare repository, not the historical Mac `56278e3` state. Six live docs yielded seven H-table
hypotheses and six source artifacts. Five exact H declaration/discussion headings remain
explicit prose exceptions. No R/E definition rows or journal DB were found under the
selected Linux boundaries; the report carries `journal-unavailable`. The live source API
was inspected read-only: `TradeJournal`, `trade_intents`, `trade_outcomes`. Portable fixtures
exercise H/E/R rows and real SQLite intent/outcome tables, including a live WAL test. The
owner must locate/export the actual journal on its owning host, decide R/E mappings, pin
empirical protocols and verify chronology. No imported timestamp fabricates preregistration.

**Orrery:** 39 `sim.json` catalogs become programs, never completed experiments. All 42
result/run JSON containers remain source/result artifacts with explicit semantic-mapping
exceptions; eight repeated anonymous content identities are withheld as collisions,
leaving 34 artifact candidates. Their complete raw records remain in the external report.
The 16,527 nested object members are individually inventoried, including realizations and
aggregates. The owner must supply format-specific run identities, identify repeated result
copies, recover protocol/input pins, and separate execution, control diagnostics and
assessments. The wide-binary result decision/control fields were inspected but never
converted into confirmatory primary evidence by a generic JSON importer.

**Astrolabe:** 27 sidecars and all 27 matching Parquet files were hashed without loading
Parquet into memory or importing Astrolabe. Sixteen sidecar units have no remaining mapping
exception; 11 carry pending lineage and 23 structured lineage entries remain explicit
member exceptions. In particular, the crossmatch sidecar explicitly says a Gaia input
was never persisted; that missing parent remains null. Overwrite-by-name and fetched-at
alone do not establish historical snapshot identity. The owner must publish immutable
snapshot pins and resolve kind/name ambiguity and historical lineage, preserving missing
parents. The current digest proves inspected bytes, not which bytes an older analysis used.

## Reproduction and validation

Use the [installation commands](../README.md), then run the same command for each row
above, substituting its repository and exact revision:

```bash
/tmp/research-env/bin/orbit-research import principia --source-root /home/daniel/workspace/constellation/codebases/principia --repository principia --expect-revision 13866b2848fbcce9a1f1ef2d4b97051a65f7e626 --dry-run --output /tmp/principia-live-report.json
/tmp/research-env/bin/orbit-research validate /tmp/principia-live-report.json
```

The required exact revision makes future checkout changes a visible mismatch. Ignored
external datasets are additionally pinned by SHA-256 in the report; refreshing them will
produce different file/report digests even when Git HEAD stays the same.

Validation used a fresh isolated Python 3.12.3 environment with the built package installed
(non-editable), jsonschema 4.26.0 and referencing 0.37.0. All 28 focused unittest cases passed,
including four-adapter CLI imports/validation, exact field/verdict preservation, malformed
JSON, collisions, dirty/revision provenance, missing lineage, output guards, source-tree
byte equality, SQLite WAL visibility, semantic protocol revisions, pending reconciliation,
failed-control inference and model/nature separation. The generated synthetic example
report also passed the installed CLI validator. The runner's default uv cache was read-only;
a temporary cache outside the checkout allowed the full install and checks to run. No
validation was skipped.

This proves the bounded contract/import foundation. Owner reconciliation, authored frozen
protocols, native commands, projections/browser, pilots and cutovers remain separate work.
