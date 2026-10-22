# Pitcrew

**A runner manager for Gitea and Forgejo.** Pitcrew installs, runs, watches and updates the CI runners on your build
machines — Gitea's runner (`gitea-runner`, formerly `act_runner`) first, Forgejo's runner next, and later GitHub's
self-hosted runner — on **Windows and macOS** (Linux later).

A pit crew keeps the car going without stopping the race: Pitcrew restarts a runner that crashed or stalled, and
updates it between jobs, so the builds keep running.

> **Status: early skeleton (v0.1.0, unreleased).** The design is written down and the pure core is built and tested;
> the service and the app are scaffolds that build and answer "not implemented yet". See [What works](#what-works).

## Why

Keeping a self-hosted runner healthy usually ends up as a pile of scripts: a scheduled task that starts it at log-on,
a supervisor loop that restarts it with backoff and rotates its log, a watchdog that restarts it when its polls keep
timing out, and a daily job that downloads a new binary and swaps it when no job runs. That works until it doesn't,
and nobody can see what it is doing. Pitcrew does the same work as a proper service, with a window and a tray icon
that show what is going on, and adds what the scripts lack: roll-back of bad updates, crash-loop protection, several
runners per machine and macOS support.

## What it does (design)

1. **Runs the runner as a service.** A small Rust service: a Windows service under the Service Control Manager, or a
   per-user launch agent on macOS ([ADR 0003](docs/adr/0003-service-model.md)). On Windows each runner runs either
   in the logged-on user's desktop session (default; jobs that need a desktop, such as WebView2 tests, work) or in the
   service session. Registering and removing the service is done from the app; elevation prompts are the OS's own,
   and Pitcrew never handles passwords.
2. **Health and watchdog.** Per runner: process alive, registered with the server, polling OK, job running. A
   runner whose polls keep timing out while no job runs is **stalled** (default: 6 timed-out polls in 15 minutes)
   and is restarted. Restarts back off (10 s doubling to 5 min, reset after 10 min of running); 5 exits in 10
   minutes is a **crash loop**: restarts pause for 30 minutes and you are notified. A running job is never killed.
3. **Runner auto-update** ([ADR 0002](docs/adr/0002-runner-auto-update.md)). Checks the official releases
   (`dl.gitea.com/gitea-runner`, cross-checked with the gitea.com release) on a schedule, downloads the binary for the
   platform, verifies its SHA-256 from two hosts (signatures where a project publishes them), waits until the runner
   is idle, swaps the binary, restarts it and waits for the server to accept it — and **rolls back** to the previous
   binary automatically if it doesn't within a few minutes. Policy per runner: automatic / notify only / manual, an
   optional pinned version, and an update window. The window shows current and latest version and the last update's
   result.
4. **Pitcrew updates itself** with the Tauri updater: minisign-signed releases, opt-in ([docs/releasing.md](docs/releasing.md)).
5. **Tray app and window.** Status per runner, logs (bounded, searchable), start / stop / restart, settings, and
   notifications for failures and updates. Closing the window keeps Pitcrew in the tray; quitting the app never stops
   the runners.
6. **Several runners per machine**, each with its own folder, labels and work directories, configured in a versioned
   `pitcrew.toml` that tolerates damage (falls back to the last good copy, skips one bad entry).

The architecture (a pure core, a privileged service, an unprivileged app, and the local IPC between them) is in
[ADR 0001](docs/adr/0001-architecture.md).

## What works

| Area | State |
| --- | --- |
| Core rules (`crates/pitcrew-core`) | **Built and unit-tested:** log-line classification, stall rule, restart backoff and crash-loop protection, health status, update policy (modes, pin, window across midnight), update state machine with drain/postpone/roll-back, versions, release locations for Gitea's and Forgejo's runner, checksum files and verification, configuration with migration and damage tolerance, app ↔ service protocol and framing, launch agent plist. |
| Service (`crates/pitcrew-service`) | Command line, configuration loading, child processes with timeouts. On Windows: runs under the SCM (start/stop) and `install`/`uninstall` register it with automatic delayed start and recovery actions — **compiled, not yet tried on a real machine**. Supervising runners, the IPC server, downloads and updates: *not implemented yet*. macOS launch agent registration: *not implemented yet* (the plist is rendered). |
| App (`app/src-tauri`, `app/src`) | Tray icon (Open / Quit), window that hides to the tray, the status view (against the mock in the browser build), "updates not available in this build" without an update key. The service client: *not implemented yet*, so the real app shows "Connecting to the Pitcrew service is not implemented yet" with Retry. |
| Installers | CI builds a Windows NSIS installer on tags and nightly; it does not contain the service yet. |

## Roadmap

1. Service: the supervisor loop for one runner on Windows (start with redirected logs, classify lines, stall rule,
   backoff, graceful stop), the named-pipe server and the app's client; status in the window and tray.
2. Interactive-session launch on Windows (ADR 0003), session-change handling; service mode.
3. Runner auto-update end to end (download, two-host checksum, drain, swap, confirm, roll-back) with a fake runner
   binary in integration tests.
4. Bundle the service with the app; install/uninstall from the app with UAC; the macOS launch agent via
   `SMAppService`.
5. Logs view (paged, searchable), settings, notifications; register a runner from the app.
6. Pitcrew self-update UI (check, download, restart) on top of the wired updater plugin.
7. Forgejo's runner (user-provided binary on Windows/macOS, OpenPGP-verified updates on Linux); Linux (systemd).
8. Later: GitHub's self-hosted runner, LaunchDaemon mode for headless Macs.

Dedicated build Macs: since the macOS service is a launch agent, enable automatic log-in for the CI user so the
runners start after a reboot.

## Building

Prerequisites: Rust stable (1.88+), Node 24, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)
for your OS (WebView2 on Windows, Xcode command line tools on macOS; on Linux `libwebkit2gtk-4.1-dev` and friends).

```sh
cd app && npm ci && cd ..
cargo test --workspace                 # core, service and app tests
(cd app && npx ng test --watch=false)  # UI tests
(cd app && npm start)                  # UI in a browser, against the mock backend
(cd app && npx tauri dev)              # the desktop app
cargo run -p pitcrew-service -- check-config
```

More in [docs/development.md](docs/development.md); releasing and the update signing key in
[docs/releasing.md](docs/releasing.md); engineering rules in [AGENTS.md](AGENTS.md).

## Name and trademarks

"Gitea" and "Forgejo" are trademarks of their respective owners. Pitcrew is an independent project and not
affiliated with or endorsed by either; it manages their runners, which speak the same protocol.

## Licence

Pitcrew is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
