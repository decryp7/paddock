# 3. Service model: a Windows service that can start runners in the user's session; a macOS launch agent

- Status: accepted (2026-10-22)
- Related: ADR 0001 (architecture), ADR 0002 (runner auto-update); code: `pitcrew_service::platform`,
  `pitcrew_core::service_spec`, `pitcrew_core::config::SessionMode`

## Context

The user wants the runner run "as a proper service": on Windows under the Service Control Manager (no scheduled
task), on macOS under launchd, registered and removed from the app, with elevation prompts that are the OS's own and
no passwords handled by Pitcrew.

One hard constraint comes from the machine Pitcrew replaces: the runner was moved **out** of a Windows service
(WinSW) into a log-on scheduled task because jobs there need an interactive desktop: the real-app smoke tests use
WebView2, which does not work in session 0 where services run, nor as SYSTEM (its temp folder is
`C:\Windows\SystemTemp`). The same holds on macOS for anything that needs the user's GUI session or login keychain
(UI tests, simulators, WebKit, code signing).

## Decision

### Windows: one Pitcrew service under the SCM

- `pitcrew-service.exe` is a real Windows service (Rust, [`windows-service`](https://github.com/mullvad/windows-service-rs)
  0.8, MIT/Apache-2.0, maintained by Mullvad: safe wrappers for the dispatcher, control handler and service manager).
  Name `Pitcrew`, display name "Pitcrew runner manager", own process, **automatic (delayed) start**, account
  **LocalSystem**.
- Recovery actions: restart after 10 s, 60 s, then 5 min; failure count reset after a day; applied to non-crash
  failures too. Inside the service the runners have their own supervision (backoff, crash-loop protection).
- `pitcrew-service install` / `uninstall` register and remove it; the app runs them through UAC (`runas`), so the
  prompt is Windows' own. Removing a service that is not installed succeeds; installing twice says so.
- Each runner has a **session mode** (`session` in `pitcrew.toml`):
  - **`interactive`** (default on Windows): the service starts the runner **in the session of the user logged on at
    the console**, with that user's token (`WTSGetActiveConsoleSessionId` + `WTSQueryUserToken`, which needs
    LocalSystem, then `CreateProcessAsUserW` on `winsta0\default` with the user's environment block). No password is
    needed or stored. The service subscribes to session changes (`SERVICE_ACCEPT_SESSIONCHANGE`): it starts the
    runner at log-on and stops it gracefully at log-off, like the old task, but with the SCM's status, recovery and
    Pitcrew's watchdog. Jobs that need a desktop (WebView2) work.
  - **`service`**: the runner runs in session 0 as a child of the service, before anyone logs on, without a desktop.
    Jobs then run as LocalSystem; the window warns about this. A later option is one SCM service per runner under a
    virtual account (`NT SERVICE\Pitcrew-<name>`), which the SCM creates without a password.
- Graceful stop: the runner shuts down on Ctrl+C/Ctrl+Break (it then waits for running jobs). Pitcrew starts it in
  its own hidden console and process group, and stops it by sending Ctrl+Break to that group (a small helper
  attaches to the runner's console to do so, since a service has none). Only after the stop timeout, and only when
  no job runs, is the process tree terminated.
- Standard output and error go to the runner's log file through pipes the service reads (so the watchdog sees every
  line), never through `cmd.exe` (the PowerShell set-up needed `cmd` because Windows PowerShell 5 garbled stderr).

### macOS: a per-user launch agent, not a launch daemon

- The service is a **LaunchAgent** of the user who runs the runners: label `net.decryptology.pitcrew.service`,
  `ProgramArguments = [pitcrew-service, run]`, `RunAtLoad`, `KeepAlive`, `ThrottleInterval 10`,
  `LimitLoadToSessionType Aqua`, logs to `~/Library/Logs/Pitcrew/service.log` (rendered by
  `service_spec::launch_agent_plist`).
- Registered with **`SMAppService.agent(plistName:)`** (macOS 13+) from the plist inside the app bundle
  (`Contents/Library/LaunchAgents/`); the user approves it in System Settings ▸ General ▸ Login Items, which is the
  OS's own prompt. Development builds outside a bundle use `launchctl bootstrap gui/<uid>` on
  `~/Library/LaunchAgents/net.decryptology.pitcrew.service.plist`.
- **Why an agent:**
  - Runners and their jobs get the user's GUI session, keychain and environment, which macOS CI work commonly needs
    (UI tests, simulators, WebKit, code signing); a daemon has none of these.
  - It needs no root: a daemon runs as root unless `UserName` is set, and running CI jobs as root is far more
    privilege than they need. Installing and updating an agent needs no administrator rights.
  - It matches the Windows default (runners in the user's session) and GitHub's own self-hosted runner, whose
    `svc.sh` installs a LaunchAgent for the same reasons.
- **Trade-off:** the agent runs only while the user is logged in. Dedicated build Macs should log in automatically
  (documented in the README). A **LaunchDaemon mode** (`/Library/LaunchDaemons`, `UserName` set to the CI user,
  installed with the administrator's authorization) is the planned option for headless hosts whose jobs need no GUI.

### Linux (later)

systemd: a user unit with lingering for GUI-less runners in the user's account, or a system unit with `User=`.

## Consequences

- The Windows service is privileged (LocalSystem) because starting processes in another session requires it; its
  surface (the pipe protocol) stays small and validated (ADR 0001).
- On Windows the runner no longer depends on a scheduled task, and the SCM shows whether Pitcrew runs; on macOS
  launchd keeps the agent alive.
- Interactive mode needs a logged-on user, exactly like the set-up it replaces; service mode does not but cannot run
  desktop jobs. The choice is per runner.
- The Win32 session APIs (`WTSQueryUserToken`, `CreateProcessAsUserW`, console control events) need `unsafe` FFI;
  it will live in one small module with its own tests on the Windows runner, and the rest of the crate stays
  `#![deny(unsafe_code)]`.
