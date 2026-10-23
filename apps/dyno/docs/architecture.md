# Dyno architecture

Dyno is built from a pure Rust core, thin OS adapters, a per-user agent that measures and keeps history, a Tauri app that
shows it, and a privileged helper that exists only after the user installed it. This page describes how the pieces fit and what is true today; the diagrams
are in [design.md](design.md) and the reasoning is in the [ADRs](adr/README.md). Anything marked *planned* is not in the
code.

## Overview

![System context](../diagrams/svg/c4-1-context.svg)

![Containers](../diagrams/svg/c4-2-containers.svg)

| Piece | Crate or path | Runs as | Responsibility |
| --- | --- | --- | --- |
| Core | `crates/dyno-core` | library | Every decision, as pure functions over values (no I/O, no clock) |
| Adapters | `crates/dyno-platform` | library | The core's ports per OS (Windows, macOS, Linux), plus fakes for tests |
| IPC | `crates/dyno-ipc` | library | The one local IPC: length-prefixed postcard frames, token handshake |
| Agent | `crates/dyno-agent` (`dyno-agent`) | user process, started at login | Sampling, SQLite history, rules, process control, request handlers |
| Helper | `crates/dyno-helper` | a system service (root / LocalSystem) installed on a click | The administrator changes: a closed set of `HelperAction`s, authenticated caller, audit log (ADR 0006, 0016) |
| Plugin API and host | `crates/dyno-plugin-api`, `dyno-plugin-host`, `plugins/` | out-of-process | Vendor sensors, benchmarks; guarded (ADR 0007) |
| Shell | `app/src-tauri` | user process | Window and tray, commands, the link to the agent, native feel |
| UI | `app/src` | webview | Angular presentation: pages, SignalStores, charts |

## The pieces

### Core

`dyno_core` holds the model (`metrics`, `process`), `grouping` (ADR 0003), `protect` (the absolute protected list),
`rules` (rule model, profiles, the `Engine` state machine, ADR 0004), `diagnosis`, `anomaly`, `forecast`, `health`,
`tuning`, `history` (tiers and min/avg/max downsampling), `sampling` (adaptive intervals), `cleanup` (catalogue and
admission policy, ADR 0005), `activity` (undo records), `helper` (action types and validation), `helper_wire` (the helper's requests, replies, audit entries and state), `helper_paths`, `startup`, `alerts`,
`report`, `issues` (OS problem events, the stability score), `unusual` (the baseline of programs), `snapshot`, `settings` (versioned), `tray` and `units` (pure formatting), and `protocol` (the app-agent
contract, `PROTOCOL_VERSION`). The ports are in `ports.rs`: `SystemSensors`, `ProcessSource`, `ProcessControl`,
`FileLocks`, `StartupItems`, `FactsSource`, `UserActivity`, with `PlatformError`. Their contract (no panics, unavailable
readings with a reason, bounded calls, start-time check on every `ProcessRef`) is documented there and binding on every
adapter.

### Adapters

`dyno_platform::platform()` wires the ports for the current OS:

- **Everywhere:** sysinfo-based sensors and process table.
- **Windows:** performance counters (PDH) behind the sensors, network, Win32 process control (priority, affinity,
  EcoQoS, suspend of every thread, named job objects for hard CPU caps) and user activity (idle time, foreground
  window). Process details (`NoDetails`), file locks (Restart Manager), startup entries and facts are **not wired yet**:
  the Windows platform still uses the in-memory fakes for them, so those screens are empty on Windows.
- **macOS:** libc signals, `setpriority` with the Darwin background policy, `lsof`, `codesign`, launch agents and login
  items for startup, `pmset`/`sw_vers` facts, CoreGraphics idle time.
- **Linux:** PSI, `/proc` and `/sys` through `HostPaths` (so tests use temporary trees), hwmon and GPU sensors, process
  details from `/proc`, locks, autostart and systemd user units, package facts, per-thread `nice` and affinity.
- **Unix control** (`unix_control.rs`) is shared by macOS and Linux: `SIGTERM`/`SIGKILL`, `SIGSTOP`/`SIGCONT`, `setpriority`,
  and CPU caps as a duty cycle of 100 ms (ADR 0004). A `ControlState` file records what outlives the call.

Windows-only and macOS-only code stays in small modules; `unsafe` is denied workspace-wide except in those modules, each
block with a `// SAFETY:` comment.

### Agent

`dyno-agent` has these parts (see the component diagram in [design.md](design.md#level-3--agent-components)):

- **`sampler`**: one thread. Each tick it reads `SystemSensors` at the system interval and `ProcessSource` at the process
  interval (`sampling::intervals`: faster when someone is watching, slower when nobody is, doubled on battery,
  stretched when the agent's own CPU exceeds its budget), groups the processes, feeds the rules `Engine`, runs the
  diagnosis and anomaly checks, records history, and pushes a `LiveUpdate`. Slow facts (`FactsSource`: disk health, pending updates, power plan) are gathered on demand for the
  health report and cached, never on this thread. A panic in one tick is caught and counted.
- **`server`**: binds the private endpoint, accepts connections (token, protocol version and, where the OS tells, peer pid),
  one thread per connection, and a bounded push queue per subscriber.
- **`handlers`**: one function per `Request`. Every action is validated, checked against the protected list, applied to
  a `ProcessRef`, and recorded as an `ActivityEntry` with its inverse. A panic becomes an internal error for that request.
  The helper requests (`HelperStatus`, `InstallHelper`, `UninstallHelper`) are answered by the helper centre.
- **`issues`** and **`unusual`**: two background threads. The first reads the platform's `IssueSource`s every 15 minutes
  (panic-contained, "unavailable: reason" per source), stores new events and raises alarms; the second compares the process
  table with the baseline every 30 s ([ADR 0017](adr/0017-issues-and-the-program-baseline.md)).
- **`store`** and **`rollup`**: the SQLite store (ADR 0008) and the `HistoryWriter` that records raw points and rolls
  closed buckets up through the minute, 15-minute and hour tiers.
- **`service`**: install, uninstall, status and `ensure` of the per-user registration (ADR 0010); the unit text is pure.
- **`discovery`**, **`paths`**, **`files`**: `agent.json`, the data-folder layout and atomic JSON writes with a backup.
- **`cleanup`**: root resolution per OS (pure). Scanning and deleting are *planned*.
- **`cli`**: `run`, `status`, `stop`, `once`, `bench`, `service …`.

### App

The shell (`app/src-tauri`) owns the window and tray lifecycle, the settings that belong to the app (launch at login),
and the **link** to the agent: it reads `agent.json`, starts an agent when none runs, keeps one `dyno-ipc` connection,
forwards the webview's requests with a 15 s timeout, fans pushes out to the windows, reconnects with backoff and
replaces an agent of another build. The UI never calls `invoke` directly: it uses the `BACKEND` token, whose real
implementation calls the shell's one `dyno` command and whose mock serves the browser build. State lives in SignalStores
(`live`, `history`, `processes`, `activity`, `alerts`, `diagnosis`, `health`, `rules`, `settings`).

### Helper and plugins

The helper (ADR 0006, 0016) is `dyno-helper`, one program with three commands: `serve` (the daemon), `install` and `uninstall`.
The agent never elevates itself: on the user's click it asks the OS to run `dyno-helper install --user … --agent-exe …`
elevated (`dyno_platform::elevate`: `osascript` on macOS, `pkexec` on Linux, `ShellExecuteExW runas` on Windows), and that
command copies the helper to a system-owned folder, writes its configuration and a token readable by the user, and registers
a LaunchDaemon, a systemd unit or a Windows service. The daemon speaks `dyno-ipc` on a fixed endpoint
(`HelperMessage`), accepts a connection only from the authorised user (kernel-reported uid; a pipe ACL on Windows) running the
recorded agent program with the token, accepts only the typed administrator `HelperAction`s, validates them again, applies them
through `dyno_platform::tuning_admin::AdminTuner`, reads the setting back and writes an audit line before and after.
On the agent side `dyno_agent::helper_link` holds the link and three wrappers that route what needs the helper
(`ElevatedTuning`, `ElevatedStartup`, `ElevatedTraffic`), and `dyno_agent::helper_centre` the lifecycle (status, install on a
click in the background, remove). Plugins are
out-of-process Rust programs speaking `dyno-ipc`; the agent starts the host on first use of the plugins page
(`Request::Plugins`) and the host guards them (timeouts, memory cap, quarantine). Plugin wiring into sensors and cleanup
proposals is minimal today ([roadmap](roadmap.md#known-gaps-in-the-foundation)).

![dyno-core components](../diagrams/svg/c4-3-components-core.svg)

## Decisions at a glance

| ADR | Decision |
| --- | --- |
| [0001](adr/0001-architecture.md) | Ports and adapters; app, agent, helper, plugins; one IPC |
| [0002](adr/0002-sampling-architecture.md) | A per-user agent samples; pushed updates; adaptive intervals; own overhead measured |
| [0003](adr/0003-process-grouping.md) | Processes grouped by application |
| [0004](adr/0004-cpu-taming-and-throttling-methods.md) | Rules with an escalation ladder; job objects and SIGSTOP duty cycles; crash recovery |
| [0005](adr/0005-cleanup-safety.md) | Admission policy, preview, re-check, recoverable deletes |
| [0006](adr/0006-privileged-helper-and-observe-only.md) | Observe-only default; helper installed on first admin use |
| [0016](adr/0016-privileged-helper-install-and-ipc.md) | The helper: a system service installed on a click, one authorised user and program, audit log |
| [0017](adr/0017-issues-and-the-program-baseline.md) | Issues from the OS logs behind a port; a baseline of programs that explains, never accuses |
| [0007](adr/0007-extensibility-three-tiers.md) | Core, TOML packs, out-of-process plugins |
| [0008](adr/0008-storage-sqlite-and-retention.md) | SQLite, tiers, versioned schema |
| [0009](adr/0009-native-feel.md) | Desktop feel in a webview |
| [0010](adr/0010-platforms-and-services.md) | Launch agent, systemd user unit, Windows Run entry |
| [0011](adr/0011-design-system.md) | Tokens, themes, a colour-vision-checked palette |

## Behaviour in text

### Live updates

The app subscribes once (`Request::Subscribe { detail }`; `Summary` for the dashboard, `Processes` while the task manager
is open). The sampler measures at the adaptive interval and the agent pushes a `LiveUpdate` (system sample, memory
pressure, top apps by CPU and memory, the agent's own overhead, the active profile, active rule actions, and, for
`Processes`, the full process list and groups) to each subscriber at its detail level. A subscriber that cannot keep up
loses frames; nothing is buffered without limit. Frames are bounded (`MAX_FRAME` 4 MiB).

### Acting on a process

`Request::Act` or `ActOnGroup` is validated (`ProcessAction::validate`), each target is checked against the protected
list, applied through `ProcessControl` (which first compares the live process's start time, so a reused pid is never
hit) and recorded with an `UndoRecord`. `Undo` applies the inverse. Rule steps work the same way, driven by the `Engine`,
and are released when the rule stops holding, when the app exits and when the agent stops (`release_all`, which also reads
the control-state file to undo what a crashed run left).

### CPU taming

See ADR 0004. The engine is a state machine per (rule, app): Watching, Acting at step n, Releasing, Snoozed. The per-OS
methods are job objects (Windows) and a 100 ms SIGSTOP/SIGCONT duty cycle (macOS, Linux), background QoS / EcoQoS / nice
for efficiency mode. After a crash, Windows keeps the named job alive through a query-only handle held by the capped
process; on Unix a process left stopped is continued by the next run's `release_all` from the state file.

### History

Raw points every 5 s for `raw_hours` (24 h), then 1-minute, 15-minute and hourly rollups with min/avg/max, kept 14 days,
90 days and 2 years by default (configurable and bounded); pruning runs hourly. A range query picks the finest tier that
covers the range and returns at most a few thousand points.

### Updates

The app updates through the Tauri updater (signed, opt-in; docs/releasing.md). After an update the shell registers the
agent service again when the OS refuses the old registration (macOS) and asks an agent of another build to exit; the
service manager brings the new one back.

## State and files

One folder per user (`DYNO_DATA_DIR` overrides it): `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno`,
`$XDG_DATA_HOME/dyno` (`~/.local/share/dyno`).

| File | Content | Written by |
| --- | --- | --- |
| `dyno.db` | History tiers, per-app minutes, disk space, events, activity with undo records, alerts, snapshots, boots (SQLite, WAL, versioned) | agent |
| `settings.json` | Versioned settings (`SCHEMA_VERSION`), atomic write with backup | agent |
| `rules.json` | Rules, protected list, profile, pause switch | agent |
| `process-control.json` | What process control applied that outlives the agent (caps, suspensions) | agent control |
| `agent.json` | Endpoint, token, pid, start time, version, protocol; owner-only; removed on exit | agent |
| `logs/` | The shell's small rotating log (`app.log`); the agent writes to stderr (the service's log: `~/Library/Logs/Dyno/agent.log` on macOS, the journal on Linux) | shell, service manager |
| `plugins/<id>/plugin.toml` | Installed plugins | user |

Process-control state is under the same folder on Windows and macOS and under `$XDG_STATE_HOME/dyno` on Linux when the
agent is started by the CLI default (the agent passes its own folder).

Every persistent file is versioned and tolerates damage: settings and rules fall back to the backup, then the defaults;
a damaged database is moved aside and recreated with an alert; when the folder is not writable the agent says so and keeps
working in memory.

## Security and robustness

- **Never trust the webview.** Every request is parsed into `Request` once at the edge and validated; the agent validates
  again. Open only what the user chose.
- **Never trust another process.** Frames are bounded; the handshake needs the per-launch token (constant-time compare)
  and, where the OS tells, the peer pid. The discovery file is owner-only.
- **Least privilege.** Nothing Dyno runs is elevated; the helper has a closed action set, serves one user and one program, validates again and
  audits every change. Tauri capabilities grant only events and window dragging; the CSP has no `unsafe-eval` and no remote
  origin; `scripts/check-capabilities.mjs` enforces it in CI.
- **Contain failures.** Per-request and per-tick panics are caught, locks recover from poisoning, plugins are guarded and
  quarantined, a failing sensor is an unavailable reading, the app restarts a dead agent and an agent without an app keeps
  sampling.
- **Bounded resources.** History retention and downsampling, detail caps, scan caps, frame size, push queues, the agent's
  own CPU budget.
- **Privacy.** No telemetry; logs hold no file contents or command lines; the history database is local, clearable and
  exportable.

## Dyno's own overhead

Dyno measures itself on every tick (`Overhead` in each `LiveUpdate`) and `dyno-agent bench [seconds]
[none|summary|processes]` reports it as JSON: `none` is nobody watching, `summary` a dashboard subscriber, `processes`
the task manager open. Measured on the 14-core M4 Mac mini with `dyno-agent bench 25 …`:

| Scenario | CPU (share of **one** core) | Resident memory |
| --- | --- | --- |
| Idle, nobody watching (`none`) | about 2.0 % | about 24 MB |
| A summary subscriber (`summary`) | about 2.7 % | about 24 MB |
| A processes subscriber (`processes`) | about 3.2 % | about 24 MB |

24 MB is 0.15 to 0.25 % of the whole machine. **The load average was high (about 55 to 85) while measuring, so these are
upper bounds**; they must be repeated on a quiet machine, on release builds, on each OS (docs/suite-lessons.md). The target
is at most about 1 % of the machine. Note that the agent reports its CPU as a share of one core (ADR 0002 section 4), so
2 % of one core on 14 cores is about 0.14 % of the machine; the sampler's budget (`CPU_BUDGET` = 1.0) is compared with the
one-core figure, which is stricter, and the adaptive intervals stretch when it is exceeded.

## Extension points

- A new OS: a new `dyno-platform` adapter behind the existing ports.
- A new cleanup category: a catalogue entry (a declarative pack later), admitted by the same policy.
- A new user-facing behaviour: a rule, a profile or a setting, not a special case.
- Native or untrusted code: an out-of-process plugin (docs/plugins.md); everything else is the core or a pack (ADR 0007).
