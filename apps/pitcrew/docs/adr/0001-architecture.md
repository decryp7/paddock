# 1. Architecture: a pure core, a privileged service, an unprivileged app

- Status: accepted (2026-10-22)
- Related: ADR 0002 (runner auto-update), ADR 0003 (service model), AGENTS.md

## Context

Pitcrew replaces a hand-made set-up on a Windows build machine: a scheduled task that starts a PowerShell
supervisor at log-on, which restarts the Gitea runner when it exits (10 s doubling to 5 min, reset after 10 min of
running), rotates its log (10 MB, five files) and, in its last version, restarts it when it stalls (at least six
"fetching a task timed out" polls within 15 minutes while no job runs); plus a daily SYSTEM task that downloads a new
runner from the gitea.com releases API, checks its `.sha256`, and swaps it when no job is running. It works, but it
is invisible (no status, no history), Windows-only, has no roll-back, and every change is a script edit on the
machine.

Pitcrew must keep runners going on Windows and macOS (Linux later), for Gitea's runner first and Forgejo's (and
later GitHub's self-hosted runner) after, with a window and a tray icon to see and control them. Supervision must
continue when nobody has the app open, and must survive the app crashing.

## Decision

### Three parts

| Part | Crate | Runs as | Does |
| --- | --- | --- | --- |
| Core | `crates/pitcrew-core` | (library) | All rules as pure functions and state machines: log-line classification, the stall rule, restart backoff and crash-loop protection, health status, update policy and the update state machine with roll-back, versions, release locations, checksum parsing and verification, the configuration schema with migration and damage tolerance, the app ↔ service protocol and its framing. No I/O, no clock: time comes in as `Timestamp` values. |
| Service | `crates/pitcrew-service` | Windows service (LocalSystem) / macOS launch agent (ADR 0003) | Owns the runners: starts each with its output redirected to its log, reads the log lines into the core's classifiers, probes for running jobs (child processes), applies the core's decisions, downloads and swaps binaries, keeps the configuration, and answers the app. |
| App | `app/src-tauri` + `app/src` (Angular) | the logged-on user, unprivileged | Tray icon and window: status per runner, logs, start/stop/restart, settings, notifications, Pitcrew's own updates. It holds no runner state and can be closed or crash at any time. |

Ports and adapters: the core is the inside; runner kinds (`distribution::RunnerKind` today, an adapter per kind
in the service), the OS service managers (`pitcrew_service::platform`) and the transport (`ServiceClient` in the
app) are adapters. The app's commands depend on the `ServiceClient` trait, never on a pipe or socket. The Angular
UI goes through the `BACKEND` injection token (Tauri backend or the mock) and keeps state in SignalStores.

### App ↔ service IPC

- **Transport:** a local stream only, never TCP. Windows: the named pipe `\\.\pipe\pitcrew` created by the service
  with a security descriptor that grants SYSTEM and Administrators full access and interactive users read/write
  (`FILE_FLAG_FIRST_PIPE_INSTANCE`, remote clients rejected). macOS: a Unix socket in the user's
  `~/Library/Application Support/Pitcrew/` (mode 0600), since the service is the user's own agent.
- **Framing:** a 4-byte little-endian length, then that many bytes of JSON (`pitcrew_core::protocol`). Frames are
  at most 1 MiB; anything larger or malformed closes the connection. JSON rather than a binary codec: the volume is
  tiny (status, log pages) and a human can read a captured frame.
- **Conversation:** request/response; the first request is `Hello { protocol }` and the service refuses another
  protocol version with a message that says to install the same Pitcrew version for both. Status changes are pulled
  (the window polls while visible, the tray every few seconds); a push channel can be added as a second request type
  without changing the framing.
- **Authorisation:** reading status and logs is allowed to every interactive user of the machine; changing a
  runner (start/stop/update/settings) requires the caller to be an administrator on Windows (the service checks the
  pipe client's token) or the agent's own user on macOS. The app never sends passwords or tokens: registration
  tokens are passed by the app to `gitea-runner register` started in the service only through the protocol's
  register request (planned), are never written to Pitcrew's files or logs, and the runner's `.runner` file is never
  read by Pitcrew (only its existence is checked).
- **Validation:** every field is parsed into typed values on arrival (`RunnerName` refuses path characters, versions
  are semantic versions, limits on lines and query length); the service re-validates everything the app may have
  validated already.

### Privileges

- The service needs LocalSystem on Windows to start runners in the logged-on user's session (ADR 0003) and to swap
  binaries under `%ProgramData%\Pitcrew`, which only administrators can write.
- The app runs unprivileged. Installing or removing the service is the only elevated step: the app starts
  `pitcrew-service install` with the OS's own elevation (UAC "runas" on Windows, the authorization dialog or
  `SMAppService` approval on macOS). Pitcrew never asks for, stores or passes a password.
- Tauri capabilities are least-privilege: the main window gets `core:event:default` and Pitcrew's own commands only;
  no file system, shell or updater plugin commands are exposed to the webview. The CSP allows no remote origins and
  no `unsafe-eval`.

### Files and state

- Configuration `pitcrew.toml` (versioned with `schema_version`; the schema is documented in `pitcrew_core::config`), in
  `%ProgramData%\Pitcrew` (Windows) or `~/Library/Application Support/Pitcrew` (macOS). Before every save the
  service copies the current file to `pitcrew.toml.bak` and writes the new one to a temporary file that is renamed
  into place. On load, a damaged file is set aside as `pitcrew.toml.damaged-<time>` and the backup (else the
  defaults) is used, with a problem the window shows; one bad runner entry or section is skipped while the rest
  loads; a file from a newer Pitcrew is used read-only.
- Each runner has its own folder (`dir`): binary (and the previous one, for roll-back), the runner's own
  configuration and registration, work folders and logs. Several runners per machine have separate folders, labels
  and logs; at most 16.
- Logs are bounded everywhere: runner log files rotate (10 MB, five files by default), the service keeps a bounded
  tail per runner (by lines and bytes) for the log view, and log search is plain text (no user-supplied patterns).
  Pitcrew's own logs contain no runner log contents and no tokens.

### Platforms

Windows 10/11 and macOS 13+ first. Linux (systemd units) later; the core already knows Linux release names and the
service builds there with "not supported yet" answers.

## Consequences

- Nearly every rule is unit-tested without processes, clocks or networks; the service and the app stay thin.
- The app can be updated, closed or crash without touching the runners; the service survives log-off (Windows) and
  is restarted by the OS when it fails.
- Two binaries ship in one installer, and the protocol between them is versioned; mismatches are reported, not
  guessed around.
- The service is privileged on Windows, so its request handling is the main attack surface: it must stay small,
  typed and bounded, and is reviewed as such.
