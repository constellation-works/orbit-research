---
title: Terminal interface contract
owner: astra
last_updated: 2026-09-20
status: Accepted
type: design
summary: Terminal interface contract implementation contract.
tags: [research-workbench]
---

# Terminal interface

Follow Orbit's terminal-interface design: commands produce structured results, one
renderer owns output policy, stdout is data and stderr is diagnostics. No per-command
TTY detection or hand-built incompatible tables. MCP stdout is exclusively JSON-RPC.

New research commands support explicit machine JSON and human output. Resolve TTY
once: auto uses compact tables interactively and plain untruncated output in pipes.
No ANSI in pipes; IDs and paths must remain copyable. Existing legacy JSON behavior
is a compatibility contract: preserve it unless a separately documented migration is
approved. Explicit output mode takes precedence over auto detection.

Errors are nonzero and structured in machine mode. Usage failures exit 2, operational
failures exit 1, success exits 0; a downstream closed pipe is quiet success. Bounded
results expose truncation/pagination explicitly. Mutation output includes request key,
record/task/run identity and durable outcome; unknown outcomes are not reported as
success or retried automatically. Global diagnostics never corrupt protocol output.

CLI and MCP operations call the same Core boundary as Web. Expose scope at process
composition; tool arguments cannot replace the configured corpus/backend. Test both
TTY-independent renderer behavior and actual subprocess stdout/error contracts.
