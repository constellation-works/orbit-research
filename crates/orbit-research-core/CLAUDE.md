# orbit-research-core

Core owns research application policy and composition. Follow the repository
[architecture](../../ARCHITECTURE.md) and owning feature contracts.

- `application/`: shared use cases, planning, receipt checks and request coordination.
- `bootstrap/`: local/configured assembly and workspace initialization delegation.
- `runtime/`: process-scoped handles; no scheduler or execution engine.
- `config/` owns backend settings; bootstrap loads them and the Orbit adapter validates them.
  Tool arguments cannot replace authority.
- `adapter/mcp.rs`: bounded stdio JSON-RPC, delegating to application operations.
- `adapter/orbit/`: external Orbit CLI protocol, compatibility and identity checks.
- `assets/skills/`: bundled research guidance. Future workflow assets are deferred.

Use local research types. Do not import Orbit utilities, types or implementation
crates. Persistence belongs in Store, passive shared values in Common, transport
protocols in CLI, Web and Core’s MCP adapter. Keep scientific support separate from execution success.
