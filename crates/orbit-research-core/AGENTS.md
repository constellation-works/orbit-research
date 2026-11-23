# orbit-research-core

Core owns research application policy and composition. Follow the repository [architecture](../../ARCHITECTURE.md) and owning feature contracts.

## Rules

- Use local research types. Do not import Orbit utilities, types or implementation crates.
- Orbit owns execution, dispatch and cancellation; Core never starts an execution engine or shells out to Orbit.
- Layering (persistence in Store, passive shared values in Common, transport protocols in CLI) follows [Core module ownership](../../ARCHITECTURE.md#core-module-ownership).
- Keep scientific support separate from execution success.
- The crate bundles no data of its own: the plugin's skill, jobs and activities live under `.orbit-plugin/`, not here.
