---
type: design
summary: "Full-value access for every flexible research list column."
---

# Reference: Detail Commands Behind Truncatable Columns

[Table rendering](../specs/table-rendering.md) requires full-value access whenever
a table shortens a field. Only research lists currently have flexible columns.

| List view | Flexible columns | Full detail command |
|-----------|------------------|---------------------|
| `orbit-research research list --corpus PATH` | TITLE, TAGS, PATH | `orbit-research research show --corpus PATH --id Q001` |

Replace Q001 with the displayed canonical Q/H/T/R identifier. Detail includes the
canonical body and metadata as well as path and identities. `--format json` on
list or show also preserves full values. Non-list detail and plain pipes are
untruncated. Keep this table current whenever adding a list projection.
