# Operator capability and MCP

## Operator capability

The plugin's mutating tools, `link` and `accept`, are refused for callers without
operator capability, and an agent inside a run is not one. From a local shell in the
corpus checkout (the Orbit workspace), set `ORBIT_OPERATOR=1` for that one command,
Orbit's explicit and audited override:

```sh orbit
cd "$corpus"
ORBIT_OPERATOR=1 orbit tool run orbit.research.link --input '{"research_id":"R001","request_key":"link-R001","title":"Investigate R001"}'
ORBIT_OPERATOR=1 orbit tool run orbit.research.accept --input '{"task_id":"<task-id>","research_id":"R001"}'
```

The read-only tools (`list`, `show`, `check`, `plan`, `version`, `validate` and the
panel sources) need no override.

## Orbit's MCP server

Over MCP, start the session as an operator (`orbit mcp serve --operator`) to call
`link` and `accept`. The tool namespace is `orbit.research` for the verified install
and `research` for a local development install, so a development install's `link` is
`research.link`.

Orbit versions that group MCP tools into toolsets put the plugin's tools in the `plugins`
toolset. An operator session includes it; any other session reaches even the read-only
tools only when started with `orbit mcp serve --toolset plugins`, and is otherwise
refused with `toolset_not_enabled`.

## The corpus MCP server

`orbit-research mcp --corpus <path>` serves one corpus over MCP stdio without Orbit.
Its tools are the `research` subcommands under the same names: `research.list`,
`research.show`, `research.check`, `research.capture`, `research.create`,
`research.revise`, `research.revise_question`, `research.assess`, `research.plan` and
`research.work_links`. It has no `link`, `validate` or `accept`: those are plugin tools.
