# orbit-research-core

Core owns research application policy and composition. Follow the repository
[architecture](../../ARCHITECTURE.md) and owning feature contracts.

- `application/`: shared use cases, planning and local request correlation.
- `bootstrap.rs`: local assembly and workspace initialization delegation.
- `runtime.rs`: process-scoped handles; no scheduler or execution engine.
- `assets/`: bundled data, including research guidance under `skills/`. The
  plugin's job and activities live in the repo-root `jobs/` and `activities/`,
  not here.

Use local research types. Do not import Orbit utilities, types or implementation
crates. Persistence belongs in Store, passive shared values in Common, transport
protocols in CLI. Keep scientific support separate from execution success.

Orbit owns execution, dispatch and cancellation directly (`orbit run show`,
`orbit task update`, `orbit run job`, `orbit run cancel`); this crate no longer
shells out to an Orbit CLI adapter. See the plugin conversion spec
(constellation `operations/research/orbit-research-plugin.md`) for the
`plan`/`link`/`validate`/`accept` design. `accept` lands in a later slice.
