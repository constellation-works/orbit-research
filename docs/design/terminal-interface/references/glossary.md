---
type: design
summary: "Research CLI rendering vocabulary and contract references."
---

# Glossary: Terminal Interface

Terms with a specific meaning in Orbit Research's terminal interface. Standard
Unix and terminal terminology is excluded. Scientific record definitions belong
to the [workbench](../../research-workbench/1_overview.md).

| Term | Meaning |
|------|---------|
| Auto | Default mode resolving to a terminal table or plain pipe output. [Modes](../specs/output-modes.md). |
| Detail path | Command exposing every full value a list may shorten. [Detail commands](./detail-commands.md). |
| Fixed column | Column whose values never shorten under width pressure. [Tables](../specs/table-rendering.md). |
| Flexible column | Column allowed to shrink to eight display columns and then be dropped. [Tables](../specs/table-rendering.md). |
| Payload | Validated structured application result before presentation. [Design](../2_design.md). |
| Plain | Headerless uncolored full TSV list representation selected by auto in pipes. [Modes](../specs/output-modes.md). |
| Renderer | Layer projecting payloads into human or machine bytes. [Design](../2_design.md). |
| Sink | Resolved mode, width, terminal state and color permission. [Modes](../specs/output-modes.md). |
| Uniform suppression | Omitting selected constant columns only in auto terminal lists. [Tables](../specs/table-rendering.md). |
