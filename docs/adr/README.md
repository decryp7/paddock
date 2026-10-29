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
| [0014](0014-monorepo-under-paddock.md) | One monorepo under `paddock`: layout, history import, per-app tags and releases, one updater endpoint per app (no bridge: old installs reinstall once), affected CI, task runner (accepted) |
| [0015](0015-assistant-memory.md) | Assistant memory: per app, off until enabled, user-confirmed facts and findings, the poisoning rule, SQLite with the app (accepted) |
| [0016](0016-dialog-window-sizing-contract.md) | Dialog window sizing contract: measured content size, one dialog frame with a fixed footer, checked over every dialog at three scales (accepted) |
| [0017](0017-viewport-and-selection-survive-refreshes.md) | Viewport and selection survive data refreshes: row keys, an anchor row, selection by key, scrollbar markers (accepted) |
| [0018](0018-bounded-loading-everywhere.md) | Bounded loading everywhere: limits or pages on every query, virtualized or capped lists, streaming imports, timeline lane selection (accepted) |
| [0019](0019-limits-explain-themselves.md) | Limits explain themselves: one typed limit error with the value and the Settings path, Raise it and Retry, a test per limit (accepted) |
| [0020](0020-shared-executor.md) | One shared executor in `paddock-jobs`: tasks, periodic checks (poller pool) and leases, with priority classes, cooperative abort and a stuck watchdog |
| [0021](0021-portable-tests.md) | Portable tests: `paddock-testkit` for OS-aware paths, text, quoting, scaled fixtures, condition waits and bounded tools; check C38 over every test; xwin clippy before push; `.gitattributes` policy (accepted) |
| [0022](0022-rust-developer-and-ci-tool.md) | All tooling in Rust: one developer and CI tool, `paddock-dev` (`./dev`), content-addressed binary published from main, shared `paddock-repo-files`, `-graph`, `-rules` and `paddock-drawio` crates; six phases after the CI kit consolidation; Playwright suites, the TS analysis (until generated from Rust) and ESLint rules stay; floating `stable`, no toolchain pin |
| [0023](0023-tests-never-free-a-port-they-probe.md) | Tests never free a port they probe: `paddock_testkit::net::HeldPort` (hang up or reset), a refusing connector or a mapping test for "refused", check C39 (accepted) |
| [0024](0024-background-work-contract.md) | Background work: background priority for threads and child processes through `paddock-proc`, wake-ups instead of polls, long work as keyed jobs in the executor, per-pid monitoring; checks C40-C42 |
| [0026](0026-shared-dialog-api.md) | The shared dialog API: contracts declared in Rust with generated TypeScript, `DialogFrame` with declared actions (order, Enter, focus, busy), windows created hidden and shown at a ready handshake, `pd.confirm`, the test kit and each app's migration |
| [0027](0027-table-layers-and-sticky-headers.md) | Table layers: sticky headers above their rows (`--pd-z-table-*` ladder, conformance C43, `expectHeaderOnTop`), and `pd-table` built on Angular Material's table (sticky header, `matSort`, `mat-menu`, `matTooltip`, `matIconButton`) with group rows, middle truncation, `hideBelow`, row heights and the grid keyboard on top |
| [0029](0029-the-shell-owns-the-start.md) | The shell owns the start: `Builder::tray`, `Builder::launch` and `Builder::add_to` open the tray and the main window after every plugin is set up; windows refuse to be built inside a plugin hook; every app tests its start (`paddock_shell::testing::start`, C50) (accepted) |
