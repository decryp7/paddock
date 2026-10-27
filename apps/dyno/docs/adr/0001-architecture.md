# 1. Architecture: a pure core, OS adapters behind ports, and four kinds of process

- Status: accepted (2026-10-22)
- Related: ADR 0002 (sampling), 0006 (helper), 0007 (extensibility), 0010 (platforms and services); Debrief's ADR 0001
  and Pitcrew's ADR 0001 (the pattern this ports); `dyno_core::ports`, `dyno_core::protocol`

## Context

Dyno measures, explains and tunes a PC on Windows, macOS and Linux. Almost everything it decides (which processes form
one app, why the machine is slow, when a rule escalates, whether a file may be deleted, how a score is computed) is
logic that must be exactly the same on every OS and must be testable without a real machine. Everything it touches
(performance counters, `/proc`, Mach calls, job objects, the registry, launchd) is different per OS, unsafe, slow or
risky. Measuring has to continue while the window is closed, and nothing in the webview may hold elevated rights.

## Decision

1. **Ports and adapters.**
   - `crates/dyno-core` is pure: no I/O, no clock, no OS API. Time and measurements arrive as values. It holds the
     model (`metrics`, `process`), grouping, diagnosis, anomalies, forecasts, the rules engine, protection, cleanup
     policy, health, tuning plans, alerts, history resolution and the app-to-agent `protocol`.
   - The core's **ports** are small traits in `dyno_core::ports`: `SystemSensors`, `ProcessSource`, `ProcessControl`,
     `FileLocks`, `StartupItems`, `FactsSource`, `UserActivity`. Their documented contract (ADR 0002 and the module
     docs) is binding: no panics, unavailable readings carry a reason (`Reading::Unavailable`), calls that may block
     are bounded by a timeout, an action on a `ProcessRef` whose start time changed fails with `NotFound`.
   - `crates/dyno-platform` holds the adapters: sysinfo-based defaults for every OS plus `windows/`, `macos/` and
     `linux/` modules, the Unix control shared by macOS and Linux, and in-memory `fake`s for tests. A new OS is a new
     adapter; the core and the agent do not change.
2. **Four kinds of process.**
   - **App** (`app/src-tauri` + `app/src`): the Tauri shell and the Angular UI. It never measures. It starts or finds
     the agent, subscribes to its pushes and sends requests. It runs as the user, unprivileged.
   - **Agent** (`crates/dyno-agent`, binary `dyno-agent`): per user, started at login (ADR 0010). It owns the sampling
     loop, the SQLite history (ADR 0008), the rules engine and the process-control state, and answers the app over
     `dyno-ipc`. It is the single writer of everything on disk. It never runs elevated.
   - **Helper** (`crates/dyno-helper`, ADR 0006): a separate minimal executable with a closed set of `HelperAction`s,
     installed on first use of an action that needs administrator rights. Only the type definitions
     (`dyno_core::helper`) exist today; the executable is a stub.
   - **Plugins** (ADR 0007): out-of-process Rust, depending on `dyno-plugin-api` alone, served over the same IPC,
     guarded by `dyno-plugin-host` (timeouts, memory cap, quarantine after repeated failure).
3. **One IPC.** Every byte between Dyno processes is a `dyno-ipc` frame: a 4-byte length and a postcard body, on a
   private local endpoint (Unix socket or Windows named pipe), authenticated with a per-launch token and, where the OS
   tells, the peer's pid. App to agent, agent to plugins and later app to helper use the same crate and the same
   handshake. The webview never speaks it: the shell translates the UI's JSON commands into
   `dyno_core::protocol::Request` values and validates them (the agent validates again).
4. **The agent decides through the core, the app only shows.** Rules, diagnosis and scoring are computed in
   `dyno-core` and carried out by the agent; Angular formats and renders (AGENTS.md "Where logic lives"). The mock
   backend exists for browser development and UI tests only.
5. **Failure containment.** A panic in a request is caught and becomes an internal error for that request; a panic in
   a tick is counted and the loop continues; a failing sensor is an `Unavailable` reading; a failing plugin is
   quarantined. If the agent dies the app shows it and restarts it; if the app dies the agent keeps sampling.
6. **Contracts are versioned.** `PROTOCOL_VERSION` (app and agent), the plugin API version, `settings::SCHEMA_VERSION`
   and the database schema version each gate loading; a mismatch asks the older side to restart or migrates forward.

## Consequences

- Most behaviour is tested with plain unit tests on values. Adapters are tested on their own OS's CI runner against
  processes and files the tests create (AGENTS.md "Safety").
- Adding a feature usually means: a pure function and types in the core, a protocol request, an agent handler, a port
  method only if the OS must be asked something new, and UI. Skipping the core step is a review failure.
- Two code paths to start the agent exist (service-started and app-started); both end in the same discovery file and
  protocol (ADR 0002).
