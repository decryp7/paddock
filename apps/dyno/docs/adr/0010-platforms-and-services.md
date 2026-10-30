# 10. Platforms and services: a per-user agent on every OS, registered the OS's own way

- Status: accepted (2026-10-22)
- Related: ADR 0001, 0002, 0006, `dyno_agent::service` (`spec.rs`), `dyno_platform`, docs/macos.md, docs/linux.md;
  docs/suite-lessons.md (the pitfalls behind several rules below)

## Context

The agent must sample while the window is closed and come back after a crash, an update or a reboot, on Windows 10/11,
macOS and Linux (all three first-class). Each OS has a different way to run a background program, and the obvious
choice on Windows (a service) is the wrong one for Dyno.

## Decision

1. **The agent runs per user, in the user's session, on every OS.**
   - **macOS: a launch agent.** The bundle carries
     `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`, and the shell registers it with
     `SMAppService` (macOS 13+; approval in System Settings > Login Items when the OS asks). The plist runs
     `dyno-agent run` at login (`RunAtLoad`, `KeepAlive`, `ThrottleInterval` 10 s, `LimitLoadToSessionType Aqua`,
     `ProcessType Standard`; amended 2026-10-30: launchd runs a `Background` job as a DARWIN_BG process with every
     thread clamped to the `BACKGROUND` QoS band, and the agent answers every request of the app). The bundle is ad-hoc
     signed (`signingIdentity "-"`) because `SMAppService` and launch agents refuse a linker-signed bundle (-67056).
     After an app update launchd may keep the old code requirement and refuse the new binary ("needs LWCR update",
     spawn failed, exit 78); registering over the live job changes nothing.
     The shell therefore replaces the registration (unregister, `launchctl bootout`, wait until the job is gone,
     register, bounded retries) once per version change (the version is recorded in `service-registered-version` in
     the data folder) and whenever `launchctl print` reports it stale while the agent is silent
     (`service_repair::plan`, `run_repair`; Paddock rule 24). Outside a bundle (development, the CLI) `dyno-agent
     service install` writes `~/Library/LaunchAgents/net.decryptology.dyno.agent.plist` and runs `launchctl bootout`,
     waits (bounded) until launchd dropped the job, then `bootstrap`, logging to `~/Library/Logs/Dyno/agent.log`.
   - **Linux: a systemd user unit.** `~/.config/systemd/user/dyno-agent.service`, `Restart=always`, `RestartSec=5`,
     `Nice=10`, `CPUWeight=20`, `IOSchedulingClass=best-effort` with `IOSchedulingPriority=7` (never the idle class,
     rule 52: the agent reads the folders of a scan the person watches), `MemoryHigh=256M`, `WantedBy=default.target`,
     enabled with `systemctl --user enable --now`. Without systemd the app starts the agent itself (ADR 0002).
   - **Windows: a per-user `Run` entry, not a service.** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
     `Dyno`, data `"<exe>" run`.
2. **Why not a Windows service (SCM).** Services run in session 0, isolated from the desktop since Vista. From there the
   agent could not read the user's idle time (`GetLastInputInfo`), the foreground window, the user's `HKCU` startup
   items, their `%LOCALAPPDATA%` data folder or their process list as the user sees it, and it would run as
   LocalSystem, which is the opposite of ADR 0006. A per-user Run entry starts the agent in the right session at
   logon with the right rights. The **LocalSystem piece is the helper** (ADR 0006), started on demand and later, never
   the sampler. Trade-off: no automatic restart after a crash until the next logon; the app restarts a dead agent when
   it is open, and a later hardening can add a Task Scheduler "on failure" restart.
3. **Never self-installing.** `ensure` re-registers a service that is installed but stale (another executable, or
   launchd unable to spawn it); a service that is not installed stays not installed. Installing is the user's choice
   in Settings or the first-run prompt.
4. **Update flow.** An app update replaces the executables; the app calls `dyno-agent service ensure`, and the discovery
   file's `version` lets a newer app ask an older agent to `Shutdown` so the service manager brings back the new one.
   The restart happens when idle (no action in flight).
5. **Remote Desktop and multiple sessions (Windows).** The user's session is not always the console session. Anything
   that needs the user's session enumerates sessions (WTSEnumerateSessionsW; prefer active, then console, then lowest
   id). Per-user agents each serve their own session.
6. **Linux: threads are not processes.** sysinfo lists threads as processes on Linux; every adapter filters
   `thread_kind()`, and nothing ever adopts or signals a TID as a process.
7. **Target-OS path rules.** A pure function that takes a target OS (cleanup roots, startup locations, protected
   paths) builds and compares paths with *that OS's* rules, not the host's `Path`: it must be testable on any CI
   host. `dyno_core::cleanup::policy` normalises separators and case itself; `dyno_agent::cleanup::roots` takes an
   `Env` instead of reading the host.
8. **Windows console programs** get a `conhost.exe` child: process counts and trees differ from Unix, and a helper
   started with the parent is never treated as the child's own work (grouping and rules).
9. **Login-shell PATH for anything the agent starts.** launchd and systemd start with a minimal `PATH`. The agent
   calls external tools through `dyno_platform::process` (absolute system paths where possible, timeouts, no console
   window on Windows). Anything user-configured that the agent or a plugin must start (a backup tool, a pack's
   probe) receives the user's **login-shell PATH**, obtained once (bounded, timed out, cached), plus a validated
   per-item environment override. This is the rule; the shell-PATH resolver itself is not built yet (no feature starts
   user tools today).
10. **Where files live.** `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno`, `$XDG_DATA_HOME/dyno`
    (`~/.local/share/dyno`); process-control state under `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno`,
    `$XDG_STATE_HOME/dyno`. `DYNO_DATA_DIR` overrides the data folder (tests use it, so none touches a real profile).

## Consequences

- The agent works the same everywhere; only registration and a few adapters differ.
- On Windows a crashed agent stays down until the app or the next logon; documented in docs/architecture.md.
- Per-OS verification commands (`dyno-agent service status`, `launchctl print`, `systemctl --user status`) are in
  docs/macos.md and docs/linux.md.
