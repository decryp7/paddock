# Changelog

## [1.1.0] - unreleased

### Upgrading from 1.0.x

Pitwall 1.0.x checks the old update feed and trusts the old signing key (`92556abcfacde1d1`); 1.1.0 is signed with the
Paddock family key (`9a20012beece5338`). **Install 1.1.0 once by hand** from the releases page over your 1.0.x copy: the
in-app updater of 1.0.x cannot see or verify it. Your settings, keys, models and history are kept. From 1.1.0 on, updates
arrive in the app as usual.

### Security

- Windows: the Pitwall service, which runs as the system, is registered only for a Pitwall installed directly in Program
  Files and protected from other accounts, and never for another copy. The installer shows why when it refuses, and what to do
  (uninstall Pitwall, then install it again into Program Files).
- Windows: the Pitwall service no longer writes `%ProgramData%\Pitwall\logs\service.log`. A standard user who made that
  folder first could have had the system delete any file through it (1.0.x shipped this). The service writes to the
  Application event log instead (Event Viewer, source Pitwall). Settings > Service shows why an install or Repair is
  refused before Windows asks for permission, and a registration of another copy that is not safe to run as the system is removed.

- Windows: the installer checks the install folder before it runs any program of it. A Pitwall chosen into a folder other
  accounts can write (the directory page, `/D=` or passive mode) is refused before the service is touched, and so is a fresh
  install over a copy installed for one user only. The uninstaller runs a program of its folder only when that is the folder the
  installer registered, directly under Program Files, so a copied `uninstall.exe` does nothing elevated. Settings > Service
  (Install, Repair, Remove) works only from Pitwall installed in Program Files.
- If you still have Pitwall 1.0.x installed in a folder other than Program Files, uninstall it first: its own uninstaller runs
  before any of this.

### Changed

- Windows: Pitwall installs once for every user, into Program Files, on the family's shared installer (ADR 0053). An update or
  uninstall closes Pitwall for every signed-in user. Updates install through one Windows administrator prompt; "install when
  Pitwall quits" is gone on Windows. Start at log-in is always the user's own entry, never the machine-wide one.

### Added

- Report a problem (Help menu, command palette): a diagnostics zip you review file by file and save where you choose;
  nothing is sent. It holds the system facts, your settings, the recent logs of the app, the server and the command line,
  the server's settings, which engines and model files are installed (names and sizes), and what the server and the
  background service say about themselves. Chats, attachments, presets, keys and the model files themselves are never in
  it, and keys that reach a log (`pw_…`, `pmk_…`) are removed from it (replaced by `[redacted]`). The Logging settings page sets how much the app
  logs.
- Long operations are jobs: loading a model, a benchmark, the model update check, installing and removing the background
  service, Repair and the app update run in the background with phase-labelled progress (a growing count when the total is
  unknown), Cancel in every phase, "Stop waiting" when Cancel did not take effect, and no time limit in the window: a job
  that is still running is still working.
- The memory footprint of the app and the engines is read through the shared process crate.

### Fixed

- Repair on macOS 26 works again: it reads the service state through the shared Paddock service crate instead of Pitwall's own
  `launchctl print` parser.
- A cancelled Repair whose registration cannot be restored is reported as a failure, not as a clean cancel; one Cancel stops
  every operation that shares the id.

### Changed

- The old blocking commands for these operations are gone; the window uses the jobs only.

### Removed

- Offline mode is removed (the family's "no Offline mode", 2026-11-03): Pitwall's AI runs locally, downloads happen only
  when you ask, and the model update check stays off until you turn it on (ADR 0017). A 1.0 settings file with
  `network.offline` still loads; the key is dropped on the next save.
