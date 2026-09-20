---
type: design
summary: "Single-line borderless research tables with full stable pipe output."
---

# Spec: Table Rendering

A table is a header plus one physical line per record. Its plain representation
has no header and contains full, stable tab-separated fields.

## Why This Exists

Wrapped rows and shifting pipe columns prevent reliable scanning and extraction.
Shortened values are acceptable only with a [full detail path](../references/detail-commands.md).

## 1. Structure

Use uppercase headers, two-space gutters, no borders or footer, and left-aligned
text. Research columns are ID, KIND, STATUS, TITLE, TAGS, PATH, in that order.
Missing table cells use `-`; plain missing fields are empty. Escape embedded tabs,
newlines, carriage returns and other controls so a record stays on one line.
JSON remains the lossless interchange for distinguishing literal escape text.

Empty human lists leave stdout empty and emit `No research records found.` on
stderr. A single record still uses the list layout. Counts and warnings never
become body rows.

## 2. Width and Truncation

Compute natural Unicode display widths from header and cells. A sink width of 0
means unbounded. When width is constrained, shrink flexible TITLE, TAGS and PATH
columns widest-first to eight display columns, then drop them from the right
until the table fits. Name dropped columns on stderr. Fixed ID, KIND and STATUS
never shrink, even if the terminal cannot contain them.

Truncate prose at the tail and paths in the middle with a visible `…`. Never wrap.
Do not truncate plain, JSON or NDJSON output. Character-boundary truncation is the
current implementation; grapheme-preserving truncation remains an explicit
limitation in [design](../2_design.md#8-concerns--honest-limitations).

## 3. Column Selection

Auto terminal mode suppresses uniform KIND, STATUS, TAGS and PATH columns. Always
retain ID and TITLE. Explicit table mode disables uniform suppression. Plain
output always has all six fields, including empty ones. If filters are introduced,
a filtered column must remain visible even when uniform.

## 4. Extending a View

Every new flexible column needs a documented full-detail command. Future numeric
columns align right and put units in headers. Do not add per-command padding,
TTY probes or alternate table libraries. Changing plain field order is a CLI
contract change and needs explicit migration documentation.
