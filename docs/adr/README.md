# Architecture decision records (Paddock shared crates)

| # | Decision |
|---|---|
| [0001](0001-plugin-platform-crates.md) | The `paddock-plugin` family: IPC, host, guard, packaging, permissions, versioning |
| [0002](0002-declarative-views.md) | Declarative view contributions rendered by host apps |
| [0003](0003-debrief-migration-and-users.md) | Debrief migration phases, compatibility shim, who uses what |
| [0004](0004-paddock-pitwall-client.md) | `paddock-pitwall`: discovery, pairing, requests, "Pitwall is optional" |
| [0005](0005-paddock-testplan.md) | `paddock-testplan`: `.sighterplan` / `.sighterresult` |
| [0006](0006-rollout-versioning-consumption.md) | Rollout order, other crates, versioning, consumption, open questions |
| [0007](0007-mcp-servers-across-the-family.md) | MCP servers across the family: who serves, who hosts, `paddock-mcp`, shared safety rules, rollout |
| [0008](0008-long-operations.md) | Long operations: phases, progress, cancel and parallel work (`paddock-jobs`, `pd-job`) |
