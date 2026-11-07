# Changelog

## [1.1.0] - unreleased

### Upgrading from 1.0.x

Pitwall 1.0.x checks the old update feed and trusts the old signing key (`92556abcfacde1d1`); 1.1.0 is signed with the
Paddock family key (`9a20012beece5338`). **Install 1.1.0 once by hand** from the releases page over your 1.0.x copy: the
in-app updater of 1.0.x cannot see or verify it. Your settings, keys, models and history are kept. From 1.1.0 on, updates
arrive in the app as usual.

**The admin key is no longer in the OS credential store.** Pitwall 1.0.x kept a copy of its admin key in the macOS
Keychain (Windows Credential Manager, Secret Service on Linux). 1.1.0 and the `pitwall` command line read only the
server's `admin.key` in the data folder, which already held the key, so nothing changes for you and no password dialog
appears. 1.1.0 never reads or deletes the old item. To remove it by hand, on macOS run in Terminal:

```sh
security delete-generic-password -s "Pitwall" -a "admin key"
```

On Windows, delete the generic credential `admin key.Pitwall` in Credential Manager (Windows Credentials), or run
`cmdkey /delete:"admin key.Pitwall"`. On Linux, `secret-tool clear service Pitwall username "admin key"`. Removing it is
optional: nothing reads it.

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

### Security

- The admin key lives only in `admin.key` in the data folder, on every OS (ADR 0057, rule 72: no Pitwall process uses the
  macOS Keychain). The file is owner-only from its first byte: on macOS and Linux mode 0600 in a 0700 data folder, on
  Windows a protected access list that grants only you. A key file others could read, a link in its place or another
  user's file is not trusted: the server replaces the key and logs why. The key is plain text in that file: other users
  cannot read it, but any program running as you can, as with `~/.netrc` (see the user guide, "Your data").
- An app that pairs again after losing its key can send `"rePair": true`; approving such a request revokes that app's
  older keys, so the key it lost does not stay valid. The request says so in Pitwall. A pairing without the flag (a
  second machine under the same name) keeps the older keys.

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
