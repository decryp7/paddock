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
11. **One agent, and who runs it (amended 2026-10-30).** The agent holds an OS lock on `agent.lock` in its data folder;
    a second one exits at once with code 3. While launchd (`KeepAlive`) or systemd (`Restart=always`) supervises the
    agent, the app starts none of its own and waits for the service's. Turning Launch at login on while the app's own
    agent runs hands over: the app asks its agent to exit and ends that process (only the one it started, rule 53), and
    the service's agent, refused until then, takes over at its next restart (launchd's `ThrottleInterval` 10 s,
    systemd's `RestartSec` 5 s). Turning it off: the app asks the former service's agent to exit over its connection
    (never a kill) and starts its own. A Windows `Run` entry starts the agent only at the next logon, so it supervises
    nothing now and the app keeps its own agent (which Quit then leaves running, as before).
    Waits are judged by run state (ADR 0031), and none ends on a clock, but each long one says why and offers Retry:
    after about 5 s "still starting" (the agent this app started has not written its discovery file: Retry ends that
    process through its handle and starts again), "waiting for the login service" (Retry starts the app's own agent) and
    "another agent holds the data folder" (Retry runs the connection steps again). A service that supervises on paper
    but runs nothing (macOS: SMAppService enabled, not switched off in Login Items, and the job loaded in launchd;
    Linux: `systemctl --user is-active`, `active` or `activating`) is told by the lock: when no process held it across
    four rechecks in a row (about 20 s, longer than launchd's 10 s throttle), the app starts its own agent and says so,
    and does not hand it over again until the service shows it works. A discovery file counts as ready only if it was
    written after the wait began or its agent answers; a refusal or a missing lock is written to `logs/agent.log` too.
    Follow-ups, not yet done: kickstart the service after a hand-over instead of waiting for its throttle; probe
    `RequiresApproval` again so an approval given later is noticed; slow the `is_held` probe in the waits (it takes and
    releases the lock at every look); the `/tmp/Dyno` fallback folder for an unwritable data folder. For 1.0.1: keep the service fallback visible after
    Running (an activity entry or a chip note); name the OS's own login-items place on Linux in that note; on Quit while
    the fallback is on and the service supervises, end the app's own agent so the service's takes over.

12. **Amendment 2026-10-31: the single route on macOS.** One Settings switch now means one main-app registration plus the
    existing agent service, and nothing is written under `~/Library/LaunchAgents`.
    - The app registers itself with `SMAppService.mainApp` (one "Dyno" row in Login Items); `tauri-plugin-autostart`
      is kept for Linux and Windows only (`login_item::route_for`). The old route wrote `~/Library/LaunchAgents/Dyno.plist`
      (label `Dyno`, a second row "dyno-app, Item from unidentified developer") with the path recorded at setup. On start
      the app deletes that file only when it has exactly the plugin's shape (`login_item::is_legacy_plugin_plist`: label
      `Dyno`, a `dyno-app` program, optionally `--hidden`, `RunAtLoad`, nothing else; never a link or another file).
    - Both registrations are refused (turning on only) when the executable is translocated (`/AppTranslocation/`) or under
      `/Volumes/`: the path would not exist after the next login. The message says to move Dyno to Applications.
    - A login start has no `--hidden` any more. The open-application Apple Event carries `keyAELaunchedAsLogInItem`
      (`'lgit'`; Apple's `AERegistry.h`, read 2026-10-31: "If present in a kAEOpenApplication event, application was
      launched as a login item and probably shouldn't open up untitled documents"), as a parameter or as the value of
      `keyAEPropData`; it is read through `NSAppleEventManager.currentAppleEvent` inside Tauri's setup (which runs in
      `applicationDidFinishLaunching`). The shell starts in the tray; the main window is opened right after the setup
      unless `--hidden` or the login event asked for the tray. A missed detection shows the window; it never hides one.
    - `dyno-agent service install|uninstall|status|ensure` refuses inside a `.app` on macOS (same launchd label as the
      `SMAppService` agent: `uninstall` would boot out the real agent, `status` would report "not installed").
      Outside a bundle (development) it works as before.

## Consequences

- The agent works the same everywhere; only registration and a few adapters differ.
- On Windows a crashed agent stays down until the app or the next logon; documented in docs/architecture.md.
- Per-OS verification commands (`dyno-agent service status`, `launchctl print`, `systemctl --user status`) are in
  docs/macos.md and docs/linux.md.
