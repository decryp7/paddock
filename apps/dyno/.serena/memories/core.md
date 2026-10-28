# Dyno: core map

PC health monitor, task manager with superpowers, cleanup and tuning (Windows + macOS; Linux later). Part of the user's suite (IMT/Debrief flagship, Pitcrew, Spotter, Pitwall). Pre-v1: redesign freely, no backward-compat code.

## Source map
- `crates/dyno-core`: pure logic, single source of truth (metrics model, grouping, diagnosis "why slow", anomalies, forecasts, rules engine + protect list, cleanup catalog/policy, health, tuning plans, alerts, reports, settings, history tiers, sampling intervals, tray icon pixels, app↔agent protocol). ≥ 99 % line coverage.
- `crates/dyno-core/src/ports.rs`: OS port traits; `crates/dyno-platform`: Windows/macOS adapters + `fake`.
- `crates/dyno-agent`: background process (child of the app): sampler, SQLite history (`store.rs`), JSON state files (`files.rs`), cleanup I/O, request handlers.
- `crates/dyno-ipc`: framed postcard IPC with token handshake; `crates/dyno-plugin-api`, `crates/dyno-plugin-host`, `plugins/dyno-plugin-example`, `templates/plugin`.
- `crates/dyno-helper`: the privileged helper; not shipped in 1.0 (ADR 0021/0022), planned for 1.1.
- `app/src-tauri`: Tauri 2 shell (tray with live bars, windows, one `dyno` command proxying `protocol::Request`); `app/src`: Angular UI (prefix `dy`).
- `docs/adr/`: decisions (planned, not written yet: 0001 architecture, 0002 sampling+storage, 0003 grouping, 0004 throttling, 0005 cleanup safety, 0006 helper, 0007 plugins, 0008 Pitwall integration point).

## Invariants
- Never act without an enabled rule or an explicit click; protected list absolute; foreground app never throttled; every action logged + undoable; actions target `ProcessRef` (pid + start time).
- UI ↔ backend contract: `app/src/app/core/api.types.ts` mirrored by dyno-core serde types (camelCase, externally tagged enums).
- Tests act only on processes/files/registry keys they create (the macOS CI runner is the user's real Mac).
- Commits: Conventional Commits, never Co-Authored-By/AI attribution.

Further: `mem:tech_stack`, `mem:suggested_commands`, `mem:conventions`, `mem:task_completion`.
