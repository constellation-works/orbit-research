---
type: design
summary: "Spec: Python-compatible canonical JSON for v1/v2 digests"
last_validated: 2026-09-12
---

# Spec: Canonical JSON (v1/v2)

Every `revision_id` and `protocol_digest` on a `schema_version` 1 or 2
document is SHA-256 over a specific UTF-8 JSON encoding. The Rust
canonicalizer must produce those exact bytes. A mismatch is a failed
identity, not a pretty-print difference.

## Why This Exists

Owner records, import reports, and research-view pins already hash this
encoding. A `serde_json` default would rewrite history while still
"validating."

## Encoding

Match CPython 3.11+ `json.dumps` with:

- `sort_keys=True` at every object, including nested `legacy` bags
- `separators=(',', ':')` — no whitespace
- `ensure_ascii=False` — UTF-8 in the JSON text, not `\uXXXX` for non-ASCII
- `allow_nan=False` — `NaN` / `Infinity` / `-Infinity` are errors
- output encoded as UTF-8 before SHA-256

Do not apply Unicode normalization. Do not rewrite number representations
to a shortest form. Integers that fit JSON numbers stay integers; the
Python encoder's rendering of a given `int`/`float` is the target, not
RFC 8785.

Duplicate object keys are rejected at parse time, before canonicalization.

## Digests

- Prefix: `sha256:` plus 64 lowercase hex.
- `protocol_digest(semantic)` hashes canonical bytes of `payload.semantic` only.
- Non-protocol `revision_id` hashes canonical bytes of the record with
  `revision_id`, `presentation`, and `provenance` removed.
- Protocol `revision_id` equals `protocol_digest(payload.semantic)`.
- `semantic_digest` on a protocol payload must equal `protocol_digest`.

## Failure modes

- Non-finite numbers: reject, do not coerce.
- Duplicate keys: reject, do not last-key-wins.
- `serde_json::to_vec` / pretty printers / RFC 8785 libraries: forbidden as
  the v1/v2 digest encoder.
- A digest mismatch against a golden fixture is a test failure, not a
  record rewrite.

## Migration

v1/v2 stay on this encoding for the life of those schema versions. A
different encoding requires a new `schema_version` and a documented read
path for historical bytes. See
[Python-compatible canonical JSON until a schema version bump](../4_decisions.md#python-compatible-canonical-json-until-a-schema-version-bump).

## Tests

Minimum oracle:

1. `examples/migration-report.json` candidate `revision_id` values
2. at least one native v2 export from `examples/native_workflow.py`
3. a fixture containing non-ASCII, nested `legacy` objects, and large
   integers

Golden fixtures (`examples/migration-report.json` and native-workflow
exports) are the oracle. Dual-run against a live Python install is no longer
required.

## Agent Signature

grok · 2026-09-12
