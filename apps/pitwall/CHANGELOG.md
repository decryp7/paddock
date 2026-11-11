# Changelog

## [1.1.0] - unreleased

### Upgrading from 1.0.x

Pitwall 1.0.x checks the old update feed and trusts the old signing key (`92556abcfacde1d1`); 1.1.0 is signed with the
Paddock family key (`9a20012beece5338`). **Install 1.1.0 once by hand** from the releases page over your 1.0.x copy: the
in-app updater of 1.0.x cannot see or verify it. Your settings, keys, models and history are kept. From 1.1.0 on, updates
arrive in the app as usual.

### Security

- Pairing needs the `X-Pitwall-Pair-Token` header on every poll, so apps that do not send it cannot pair: older apps (for
  example Debrief 1.1.x) must update before they can pair with this Pitwall. A poll without the header is answered `denied`
  with a note saying what to send.

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
- The admin key is replaced once on the first start of 1.1.0 (ADR 0092 W3). On a Linux machine with several accounts, 1.0.x
  wrote `admin.key` through a temporary file another account could have read for a moment, so a key made by 1.0.x is not
  kept. Apps you paired keep working (their keys are in `apps.json`) and Pitwall's window reads the new key itself. **A script
  or tool of your own that holds the old admin key must read `admin.key` in the data folder again.** A marker file,
  `admin-key-rotated-1.1`, in the data folder records that this was done; delete it to make the next start replace the key again.
- The list of apps and keys (`apps.json`) is written through a new file of an unpredictable name, owner-only from its first
  byte, so a link planted at the old fixed name (`apps.tmp`) is no longer written through (ADR 0092 W4). The backup copy
  (`apps.json.bak`) is gone: when `apps.json` is damaged Pitwall loads no apps and no keys, says so in the log with the file's
  path, and you pair your apps again; a backup could bring back a key you had revoked. A leftover `apps.json.bak` is
  removed on the first start. `apps.json` is also read only when it is trusted: a file others can read or change, a link,
  a file of another account, or (Windows) one in a folder where other accounts may write loads no apps and no keys, and
  the log says why. This matters when the data folder was moved (`--data-dir`, `PITWALL_HOME`) to a shared place.
- Pairing: the request id is 128 random bits, and a separate 256-bit poll token, given only in the answer to
  `POST /x/v1/pair`, is required to poll (header `X-Pitwall-Pair-Token`); before, the id was guessable and anyone local
  who guessed it could take the approved key. A poll without the token, with a wrong one or for an unknown or expired
  request gets the same `denied`; 5 wrong polls close the request. **An app that pairs with Pitwall must send the token**
  (Debrief and `paddock-pitwall` do); a script of your own that polled `/x/v1/pair/{id}` must add the header.
- Pitwall makes no key, pairing code or engine key when the operating system's random source fails, instead of using a
  time-seeded stand-in that could be guessed (ADR 0092 W5). The request fails with the reason in the log.

### Changed

- Windows: Pitwall installs once for every user, into Program Files, on the family's shared installer (ADR 0053). An update or
  uninstall closes Pitwall for every signed-in user. Updates install through one Windows administrator prompt; "install when
  Pitwall quits" is gone on Windows. Start at log-in is always the user's own entry, never the machine-wide one.

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

- Playground chat: images. Paste, drop or pick PNG, JPEG, WebP or GIF files (up to 25 MiB each) in the chat's question box,
  see a thumbnail of each before sending, and remove one with its x. Pitwall checks the file by its content, turns a phone
  photo upright, shrinks it to what the chosen model's vision file takes, and writes it again without its location, camera and
  other metadata; nothing is fetched and nothing leaves this computer. Only models with a vision file read images: for any
  other the chat says so in words, offers a downloaded vision model to switch to, and keeps Send off while an image waits for
  a model that cannot read it. Images stay in a saved chat, but their file names are not saved: after Pitwall restarts a sent image shows as "Image 640 × 480"; a model without vision is not sent the older ones, and the turn
  says so. The chat is still switched off in release builds until speech and the security review are done.
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
  keys, so the key it lost does not stay valid. Pitwall shows the app it has under that name and how many keys go
  ("Approving revokes all 3 keys of Debrief (debrief)"). A pairing without the flag (a second machine under the same
  name) keeps the other keys. An approval that cannot be saved now says so instead of looking done.

### Fixed

- Windows: the llama.cpp engine Pitwall downloads starts on a PC that has no Visual C++ runtime. It imports `vcruntime140.dll`
  and `msvcp140.dll`, which a clean Windows install lacks, so the engine exited with `0xC0000135` and its self-test failed
  (1.0.x has the same pin; the fix is in 1.1 only). The installer now installs Microsoft's Visual C++ Redistributable
  (`vc_redist.x64.exe`, pinned by hash) silently when the PC lacks it or has an older one (a restart is only advised), and an engine that still cannot load for that reason says so, with
  the next step, instead of "it could not start".
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
