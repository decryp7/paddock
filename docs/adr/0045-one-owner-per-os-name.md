# 45. One owner per OS name: a declared register of every OS registration name, checked over the code

- Status: **Draft (WIP, paused 2026-11-03 for Dyno 1.1.0)**; to become Proposed. Approved for drafting by the user
  (2026-11-03, ER-21 redesigns, item 7). Numbers: 0043 is expected for the dialog-sizing ADR
  (`docs/adr-dialog-size-to-content`), 0044 left as a gap; rule numbers 56 to 61 are taken on branches, 62 expected for
  that ADR, so this ADR proposes rule **64**.
- Related: ADR 0033 / rule 53 (reusable OS names: pids, ports, discovery files; this extends it), ADR 0023 (ports),
  ER-21 / rule 40, rule 55 / ADR 0034, Dyno ADR 0026 and ADR 0027.

## Evidence (read 2026-11-03)

| Instance | Source |
|---|---|
| Dyno's app (autostart plugin) and agent (`service::ensure`) both wrote HKCU `Run` value `Dyno`; `repair()` 5 s after every start overwrote the app's entry, so logon started only the agent | `8c27d153d` on `fix/dyno-windows-shell` (message); still on `main`: `apps/dyno/crates/dyno-agent/src/service/tests.rs:160` (`reg add ...\Run /v Dyno`) |
| Windows pipe squatting: any local process can create `\\.\pipe\net.decryptology.dyno.helper` first; the agent must check the server's owner is SYSTEM | Dyno ADR 0027 rev. 3, `6baa44630` on `docs/dyno-helper-1-1`, lines 159-163 |
| One launchd label for the agent job of two Dyno instances on two data folders; fixed with an FNV-1a per-folder suffix (`label_for`) | `40cab7c06` on `fix/dyno-agent-via-launchd-2` (`apps/dyno/app/src-tauri/src/link/launchd.rs`) |
| Stale-pid kills and stale discovery files | ADR 0033, rule 53 |

## Instances found so far (sweep of `main` a18e18801; incomplete)

| File | Name | Kind | State |
|---|---|---|---|
| `apps/dyno/crates/dyno-agent/src/service/spec.rs:6`, `tests.rs:160` | Run value `Dyno` (agent) | Run value | broken on main (shared with the autostart plugin's value) |
| `apps/dyno/app/src-tauri/src/lib.rs:369` | autostart plugin, Run value named after the product (unverified: auto-launch crate behaviour) | Run value | at risk |
| `apps/pitcrew/app/src-tauri/src/lib.rs:32`, `apps/pitwall/app/src-tauri/src/lib.rs:115` | autostart plugin | Run value / LaunchAgent plist | at risk (unique only by product name) |
| `apps/pitcrew/app/src-tauri/src/service_client.rs:46` | `\\.\pipe\pitcrew` opened with no SQOS flags and no server-owner check | pipe (client) | **at risk: squattable while the service is stopped** |
| `apps/pitcrew/crates/pitcrew-service/src/ipc.rs:21` | `\\.\pipe\pitcrew` (first instance) | pipe (server) | fine |
| `apps/dyno/crates/dyno-core/src/helper_paths.rs:8-14` | `net.decryptology.dyno.helper`, `dyno-helper.service`, `\\.\pipe\dyno-helper` | label, unit, pipe | at risk until ADR 0027's owner check lands |
| `crates/paddock-ipc/src/endpoint/windows.rs:209`, `apps/dyno/crates/dyno-ipc/src/endpoint/windows.rs:188` | `paddock-<random>`, `dyno-<random>` | pipe | fine (random, first instance) |
| `apps/dyno/crates/dyno-agent/src/service/mod.rs:15` | `net.decryptology.dyno.agent` | launchd label | fine (distinct from the app's on-demand job) |
| `apps/dyno/app/src-tauri/src/login_item.rs:9` | legacy `Dyno` label / `~/Library/LaunchAgents/Dyno.plist` | launchd label, plist path | legacy cleanup |
| `apps/debrief/crates/debrief-agent/src/service/mod.rs:20-24` | `debrief-agent`, `net.decryptology.debrief-agent` | service, unit, label | fine (single instance) |
| `apps/pitcrew/crates/pitcrew-core/src/service_spec.rs:7-16`; `apps/pitwall/crates/pitwall-server/src/service/spec.rs:9-17` (repeated in `service_install.rs`, `service_repair.rs`) | `Pitcrew`/`Pitwall`, `net.decryptology.*.service`, `*.service` | service, label, unit | fine, but the literals are repeated (copies) |
| `apps/shell-demo/src-tauri/agent/src/lib.rs:42` | `net.decryptology.paddock-shell-demo.agent` | label | fine |
| `tauri_plugin_single_instance` in Debrief, Dyno, Pitwall, Spotter | identifier-derived instance name | mutex/socket | to check: a second data folder or test run |
| `apps/dyno/crates/dyno-agent/src/instance.rs` | `agent.lock` in the data folder | lock | fine (per folder) |
| `tauri.conf.json` identifiers (7 apps) | app-data folder names | folder | fine; dev and test overrides to check |

## Pattern and invariant (draft)

**Pattern:** two components, two instances or a hostile process use the same OS registration name, whenever a name is a
bare literal chosen locally (Run values, Startup shortcuts, services, pipes, launchd labels and plist paths, systemd units,
mutexes and locks, app-data folders). **Invariant:** every such name has exactly one owner, declared once; where two
instances may coexist the name carries a per-instance suffix; a client of a fixed-name server verifies the server's owner.

## Decision (outline, to be written)

1. `docs/os-names.toml`: one entry per name (kind, literal or pattern, owner component, `instances = "one" | "per-folder"`,
   suffix function, `server_owner` for pipes and sockets). Constants generated or read from one module per app.
2. One shared helper in `paddock-service` / `paddock-ipc`: `instance_suffix(folder)` (the FNV-1a of `label_for`, moved
   to shared code) and a client `connect_verified(pipe, expected_owner_sid)`.
3. Conformance check (provisional **C74**): every OS-name literal of the listed kinds in Rust/TS/scripts must match a
   register entry; two entries may not share a name; a fixed-name pipe client must use the verified connect. Must fail
   today on Dyno's `Run /v Dyno` and Pitcrew's client.
4. Proposed rule 64 extends rule 53's text.

## Still to do

Work packages with agent-days, ADR 0022 G.7 placement (the register and C74 off the release path; the Pitcrew client fix
and the Dyno names inside their apps' windows), alternatives, open decisions with recommendations.
