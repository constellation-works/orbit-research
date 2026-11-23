# orbit-research-core

Core owns research application policy and composition. Follow the repository [architecture](../../ARCHITECTURE.md) and owning feature contracts.

## Rules

- Use local research types. Do not import Orbit utilities, types or implementation crates.
- Persistence belongs in Store, passive shared values in Common, transport protocols in CLI; see [Core module ownership](../../ARCHITECTURE.md#core-module-ownership).
- Orbit owns execution, dispatch and cancellation directly; this crate never shells out to Orbit.
- Keep scientific support separate from execution success.
- The crate bundles no data. The plugin's skill, job and activities live under `.orbit-plugin/`, not here.
