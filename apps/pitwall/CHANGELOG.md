# Changelog

## [1.1.0] - 2026-10-30

### Upgrading from 1.0.x

Pitwall 1.0.x checks the old update feed and trusts the old signing key (`92556abcfacde1d1`); 1.1.0 is signed with the
Paddock family key (`9a20012beece5338`). **Install 1.1.0 once by hand** from the releases page over your 1.0.x copy: the
in-app updater of 1.0.x cannot see or verify it. Your settings, keys, models and history are kept. From 1.1.0 on, updates
arrive in the app as usual.

### Added

- Report a problem (Help menu, command palette): a diagnostics zip you review file by file and save where you choose;
  nothing is sent. It holds the system facts, your settings, the recent logs of the app, the server and the command line,
  the server's settings, which engines and model files are installed (names and sizes), and what the server and the
  background service say about themselves. Chats, attachments, presets, keys and the model files themselves are never in
  it, and keys that reach a log (`pw_…`, `pmk_…`) are replaced by `***`. The Logging settings page sets how much the app
  logs.
- Long operations are jobs: loading a model, a benchmark, the model update check, installing and removing the background
  service, Repair and the app update run in the background with phase-labelled progress (a growing count when the total is
  unknown), Cancel in every phase, "Stop waiting" when Cancel did not take effect, and no time limit in the window: a job
  that is still running is still working.
- Offline mode now stops the app update check and install as well as the model update check.
- The memory footprint of the app and the engines is read through the shared process crate.

### Fixed

- Repair on macOS 26 works again: it reads the service state through the shared Paddock service crate instead of Pitwall's own
  `launchctl print` parser.
- A cancelled Repair whose registration cannot be restored is reported as a failure, not as a clean cancel; one Cancel stops
  every operation that shares the id.

### Changed

- The old blocking commands for these operations are gone; the window uses the jobs only.
