# Local evidence browser

Package 0.3 adds a disposable SQLite projection and portable JSON/HTML browser. Scientific
v1 history and v2 native appends stay in their owning repositories. The browser separates
activity, execution, controls and verdict; resolving a reference does not confirm a claim.
No server application, runtime plugin, cloud publication or operational task store is added.

## Build a portable example

Build the CLI, then point it at an operator-authored config of explicit owner checkouts.
Use fresh external paths for the database and export:

```sh
cargo build -p orbit-research-cli --bin orbit-research
./target/debug/orbit-research index --config /tmp/research-browser/index-config.json --database /tmp/research-browser/index.sqlite
./target/debug/orbit-research browse-export --config /tmp/research-browser/index-config.json --database /tmp/research-browser/index.sqlite --output /tmp/research-browser/site
python3 -m http.server 8000 --bind 127.0.0.1 --directory /tmp/research-browser
```

Open `http://127.0.0.1:8000/site/`. This is ordinary static serving, including the explicitly
mapped fixture `/orrery/` checkout; there is no API. The exported `site/` directory also works
when copied to another machine or opened directly as `index.html`. Bundled raster media
travels with it. Checkout navigation requires the documented local URL mapping on that
machine. JSON is available from the header without running the interface.

The constellation four-owner launcher is `operations/scripts/research-view.sh`. CLI tests
under `crates/orbit-research-cli/tests/index_cli.rs` cover index rebuild and browse-export
on disposable Git fixtures. Do not touch live owners, rerun a simulation or represent
synthetic numbers as new science.

## Explicit owner routing

The index configuration is an operator-authored local JSON file, separate from scientific
manifests. Paths are absolute or relative to the configuration file. Checkout roots must
be exact Git repository roots; names are stable scientific repository namespaces.

```json
{
  "schema_version": 1,
  "checkouts": {
    "principia": "/owner/principia",
    "parallax": "/owner/parallax",
    "orrery": "/owner/orrery",
    "astrolabe": "/owner/astrolabe"
  },
  "documents": [
    "/owner/principia/research/wide-binary/records",
    "/owner/principia/research/wide-binary/source-manifest.json",
    "/owner/principia/research/wide-binary/freeze-manifest.json",
    "/exports/parallax-native-export.json",
    "/exports/orrery-import-report.json",
    "/owner/astrolabe/research/datasets/records",
    "/owner/astrolabe/research/datasets/dataset-manifest.json",
    "/owner/astrolabe/research/datasets/wide-binary-chain-manifest.json"
  ]
}
```

`documents` accepts explicit individual v1/v2 scientific records, v1 manifests, validated
read-only import reports and native v2 export bundles. An explicit directory selects only
its immediate `*.json` files, in sorted order. No recursive whole-machine discovery occurs.
At least one manifest is required. Each exact record pin must be declared by a supplied
manifest to resolve. A manifest can name several repositories; multiple manifests preserve
different source snapshots of the same owner. Missing manifest targets remain visible.

For native appends, first use the existing `export` command with a full commit and explicit
`--source repository=/checkout` mappings from [native workflow](native-workflow.md). Supply
that bundle as a document. The index independently verifies its exact owner receipts and
append predecessors. Raw unpinned appends remain pending. It never chooses a newer commit
or executes a migration/import script from owner metadata. When an owner has no canonical
records yet, an explicitly generated existing-adapter dry-run report can be inspected as
historical candidate data; this does not authorize owner migration.

Outputs must remain outside every mapped owner checkout and cannot overwrite the config,
documents or input database. Document, source and output symlinks are rejected. Every
imported source path and local media path must stay under its explicit checkout. No
scientific records, manifests, simulations, private datasets or `.orbit` state are written.

## Reconciliation and evidence visibility

The snapshot key encodes `(repository, canonical ID, semantic revision, source revision)`;
record filenames and aliases do not determine identity. The index retains original record
objects, raw legacy values, source selectors, source digests and all conflicting variants.
All structural and local scientific invariants are validated before publishing the database.

Exact source SHA-256/blob pins are checked against local Git objects and working checkout
bytes. Native receipts must match their committed append chain. Changed or absent source
files leave the historical pin visible but pending. Code commits must exist in their mapped
repository. Artifact bytes need the declared snapshot digest and explicit local locator;
missing/remote/opaque or typed-descriptor artifacts remain pending. This generic index does
not load an owner parser or infer the identity of non-Git dataset parents. The typed artifact
verification library seam remains available to owning adapters, separately from this CLI.

Reconciliation is computed on private copies; canonical `reference.status` and verdicts
are unchanged. Every unresolved dependency propagates through the exact trace. Missingness,
failed controls, absent run-start chronology, unavailable results, invalid code/data pins,
mixed evidence and model/nature boundaries block primary confirmation under the existing
scientific checks. Resolving historical source bytes never promotes a historical assessment.
Only explicit native supersession selects revision heads; timestamps and file order cannot.
Assessments of superseded/inactive claims retain verdicts but are not current. Opposing
eligible assessments of one exact claim are shown as conflicting, without adjudication.

The browser searches full preserved records and aliases, filters owner/kind/open questions/
failed controls/pending/current/historical assessments, links exact source snapshots and
shows the claim, frozen terms, assessed evidence, run environment/invocation, code/data pins,
limitations and next obligations. Missing typed links are explicitly missing; source prose
is not upgraded into a fabricated canonical reference. `Original owner record` and `Full
source content` disclose the full provenance and qualifications. Import-report inventory
exceptions are preserved in JSON as well as record-level missingness in the browser.

```sh
/tmp/research-env/bin/orbit-research index-trace --database /tmp/research-browser-fixtures/index.sqlite --key '<64-hex snapshot key from the browser URL>'
```

Trace includes the selected snapshot, all assessments of that exact claim/source pin and
their transitive dependencies, including unresolved edges and recorded code/data/Orbit links.

## Atomic rebuild and recovery

Rebuilds serialize on the output parent directory on local POSIX filesystems. Validation and
reconciliation happen under that lock before a new SQLite file is populated, committed,
closed and fsynced. One atomic replacement publishes it; the directory is fsynced afterward.
Existing readers keep their previous complete database. Invalid JSON, bad digests/schema or
an error/interruption before replacement preserves the previous index. Diagnostics name
each invalid document and exact errors. Missing dependencies are valid **pending data**, not
an excuse to silently reuse an old resolved edge.

Deleting the database and rerunning `index` recreates identical logical projection content,
record keys, links and verdict visibility from unchanged inputs. `content_digest` identifies
that logical projection, not SQLite page bytes or a scientific attestation. Document/source
paths and inventory are part of the projection metadata; moving them can change the content
digest without changing scientific identities. No timestamp or random build ID enters it.

The export is a snapshot, not a watcher. Rebuild after owner changes; old exports clearly
remain snapshots. Static exports require a new destination, are assembled in a sibling
staging directory and published after all assets exist. Previous exports survive failures.
Abrupt process termination may leave a hidden `.research-index-*` or `.research-browser-*`
staging item; after verifying no rebuild/export is active, it can be removed. A failure after
atomic publication may have published the new complete output; inspect its digest before
retrying. No claim of network-filesystem or power-loss behavior beyond local POSIX fsync and
rename semantics is made.

## Trustworthy media

No links or scripts are auto-discovered from imported HTML/Markdown. Optional `media` entries
explicitly associate media with an exact indexed record reference:

```json
{
  "label": "Control apparatus",
  "role": "illustration",
  "record": {
    "repository": "orrery", "id": "urn:research:orrery:experiment:control-run",
    "revision_id": "sha256:<64 hex digits>", "source_revision": "<full commit>",
    "status": "pending"
  },
  "repository": "orrery",
  "path": "figures/control.png",
  "source_revision": "<full commit>",
  "sha256": "sha256:<64 hex digits>"
}
```

Roles are `illustration`, `empirical-evidence` or `simulation`. The role is an explicit
owner/operator assertion, never automatic scientific support. Only PNG/JPEG/WebP bytes with
matching signatures, full Git revision and SHA-256 are copied for inline display. Changed,
missing or inaccessible media gets a visible reason. SVG, HTML and scripts are never
embedded. For explicit non-raster/simulation navigation add `checkout_urls`, for example
`{"orrery":"/orrery/"}`, and serve the intended checkout at that URL path. Paths cannot
be remote, scheme-relative, encoded traversal or active schemes. Links name the mutable
checkout limitation; copied raster snapshots remain immutable within the export.

An explicitly selected HTTPS `url` instead of local media fields is shown as external and
unverified. It opens only on a click, with no referrer/opener access. It is never downloaded
or checked automatically. Imported markup is rendered with DOM text nodes; bundled data is
base64-encoded to prevent script termination/injection. Content Security Policy disables
network connections, frames, objects, inline scripts, forms and third-party resources. No
telemetry, survey download or plugin loading occurs. A user opening a simulation chooses
to navigate to its owning page; no imported simulation script runs inside the atlas.

## Validation

```sh
cargo test --workspace --locked
```

Chromium also needs its platform shared libraries (the Playwright `install-deps chromium`
command is an operator option on an appropriate machine). No browser dependency is needed
by the shipped static browser or normal framework tests.

ORB-11392 exercised all 60 Python tests and actual Chromium desktop (1440 px), narrow
(390 px) and 200% text workflows. The suite tests deterministic delete/rebuild, interrupted
publication, invalid documents, exact cross-owner traces, source changes and recovery,
missing targets, conflicts, source/output/symlink boundaries, original-byte preservation,
native positive confirmation and its withdrawal after result tampering or claim supersession,
inert script injection, opt-in media, empty states and exact absent-pin navigation.

Independent live read-only inspection on 2026-09-06 indexed 170 records (159 pending) using
Principia/Astrolabe owner record directories and manifests plus explicitly generated Parallax
and bounded Orrery dry-run reports. Mapped checkout HEADs were:

| Owner | Inspected HEAD |
| --- | --- |
| Principia | `4e3b02c59b694d85016915177ef1ae157895ed7b` |
| Parallax | `b1ecfc8573fab058da20c76343375b9cb8555bf4` |
| Orrery | `a1c430db54d585048ec85c4e7c47141db634f398` |
| Astrolabe | `993383578aa26579231bd22c90fe395d01a5f825` |

Those HEADs describe inspected checkouts; every record retains its own original source pin.
The live projection digest was
`sha256:8213ffd96f307fda82bb4786cc056c7f3dfc07afed269b30ce217e25e07fcd0d`.
Rendered live checks inspected the original wide-binary injected-effect-power assessment
(33.3% against 80%, failed controls, inconclusive) and Parallax E01 (completed, exploratory,
cannot advance H08), with full source drilldown, no remote requests and no page errors.
Private full source exports remain outside Git. Browser screenshots and machine-readable
results are attached to the Orbit task; no scientific reruns or owner edits occurred.
