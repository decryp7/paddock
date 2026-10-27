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
| [0009](0009-dialogs-kinds-and-declared-contracts.md) | Dialogs as windows: kinds and declared contracts (proposed) |
| [0010](0010-notifications-and-service-health.md) | Notifications with cause keys and a service-health model (proposed) |
| [0011](0011-layout-by-declared-constraints.md) | Layout by declared constraints: one pure layout function (proposed) |
| [0012](0012-paddock-app-shell.md) | Paddock app shell: one shell for every app (frame, startup, notifications and health, dialogs, jobs, settings, updates, menu, pages, test kit) (proposed) |
| [0013](0013-paddock-workbench.md) | Paddock workbench: the docked IDE layer on the shell for Debrief, Pacenotes and Sighter (proposed) |
| [0014](0014-monorepo-under-paddock.md) | One monorepo under `paddock`: layout, history import, per-app tags and releases, updater bridge, affected CI, task runner (accepted) |
| [0015](0015-assistant-memory.md) | Assistant memory: per app, off until enabled, user-confirmed facts and findings, the poisoning rule, SQLite with the app (accepted) |
