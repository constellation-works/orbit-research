# Plan shapes, link scope and request keys

## Plan shapes

`plan` drafts a task (title, description, acceptance criteria, `context_files`) for a
reserved R and creates nothing, in the corpus or in Orbit.

- `--shape investigation` needs `--objective`. One task owns the whole R directory.
- `--shape contribution` needs `--unit <name>` and `--objective`. The task owns the
  disjoint paths `code/<unit>/` and `artifacts/<unit>/` and leaves the R's README and
  `data/manifest.json` alone.
- `--shape synthesis` needs `--contribution <name>`, repeated for several. It
  reconciles completed contributions into the R's README and manifest, and refuses a
  contribution whose `artifacts/<name>/findings.md` is not committed.

## Link scope

`link`'s input is the plan's JSON object plus `research_id` and `request_key`. The
task's scope is the plan's `context_files`: the R directory for an investigation, one
unit's `code/<unit>/` and `artifacts/<unit>/` for a contribution, the R's README and
manifest for a synthesis.

`link` accepts exactly what `plan` derives for that R, and a unit's two paths only as a
matched pair. It refuses anything else (another unit, a path outside the R, a widened
list) with a message naming the accepted forms, and it never widens a task's scope.
Omit `context_files` and the task covers the whole R directory. Contributions to one R
with different units therefore do not hold each other's context locks.

The title must contain non-whitespace text. An omitted or blank description uses the
title.

## Request keys

A request key is 1 to 256 bytes and is bound to the scope it was first linked with.
The task is tagged `research-request:<key>`.

- An identical retry (same key, same scope) adopts the one task already tagged and
  never creates a second; its output has `created: false`.
- The same key with a different scope, or for a different R, is a conflict. Use a new key.

`orbit-research research work-links --corpus <path>` lists the links the corpus
recorded. It is a local cache, not live task status.
