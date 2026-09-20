# orbit-research — agent guide

Independent repository on agent-main. The running implementation is the Rust
workspace described by [Architecture](ARCHITECTURE.md). The approved workbench follows [its design](docs/design/research-workbench/1_overview.md)
and [crate architecture](ARCHITECTURE.md). Read the owning feature contract before
implementation; protocol adapters share Core use cases and never bypass Store guards.
Build the scientific registry and its Orbit integration; do not build a second
task engine or scheduler. Do not revive the retired Python package.

- Canonical scientific records remain owned by their scientific repositories.
- Keep claim revisions, frozen protocols, result artifacts and assessments distinct. Execution success is not scientific support. Retirement preserves verdicts and history.
- Do not edit sibling repositories from a framework implementation task. Changes there need owning-workspace tasks.
- Follow Daniel's current crew selection: Astra for architecture/design/visual work, Sol for appropriate hard numerical or implementation work, Terra for medium, Luna for low.
- Follow the root Constellation git and approval rules. Prepare validated candidates; do not merge or complete without Daniel's approval. Do not schedule agent review unless requested.
- Use current code, requirements and measured evidence. Historical ADRs and retired agent personas are not authority.
- Add focused behavioral tests for scientific invariants. The approved workbench is a separate loopback app using Observatory Markdown and an external Orbit CLI adapter.
- Never commit secrets, private source datasets or environment files. Reference large artifacts through immutable manifests and digests.
