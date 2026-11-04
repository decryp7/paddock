# 53. Per-machine Windows installs: one shared mechanism, Dyno first

- Status: Proposed (2026-11-04). The user decided the direction on 2026-11-04: Dyno 1.1.0 on Windows installs per machine, to
  `C:\Program Files\Dyno`, for all users, accepting a delay of about 1 to 1.5 weeks; start at login is always written per
  user (HKCU), even when the app runs elevated; the other apps follow later on the same mechanism (rule 42 / ER-22); **no
  migration**: a per-user copy is uninstalled by hand, and the per-machine installer refuses to run while one is there
  ("no backward compatibility before v1"). The open questions are listed under "Decisions still open" with recommended
  defaults.
- Date: 2026-11-04
- Related:
  - Dyno ADR [0027](../../apps/dyno/docs/adr/0027-helper-for-1-1-0.md) D1 (L: packaging unchanged), reversed for Windows only
    by its amendment of 2026-11-04 ([0027-amendments.md](../../apps/dyno/docs/adr/0027-amendments.md)), which adopts this
    record and holds only Dyno's parts (the helper and its orphan watch). Dyno ADR
    [0022](../../apps/dyno/docs/adr/0022-helper-installed-with-the-app.md) option W1 and §1 (per-machine NSIS that refuses
    anything outside Program Files): this record realises the per-machine install, not 0022's helper-at-install or
    helper-run updates.
  - Rules 42/43 (share first; a shared change updates its users), 53 and ADR [0033](0033-processes-by-identity-never-by-a-bare-pid.md)
    (processes by identity), 54 (a release-path change builds the installers before it lands), 55 and ADR
    [0034](0034-one-implementation-per-mechanism.md) (one implementation per mechanism), ER-14 (tests never touch the
    user's machine; VMs only), ER-19 (no UI timeouts), ER-27 (logs explain every failure). ADR 0024 (no polling), ADR
    [0031](0031-patience-not-wall-clocks.md) (bounded waits), ADR [0039](0039-release-gates-inspect-the-artifacts.md) (release
    gates inspect the artifacts), ADR [0040](0040-windows-gui-test-session.md) (Windows GUI test session), ADR
    [0042](0042-one-owner-per-harness-and-small-batches.md) (one owner per harness: `scripts/vm-smoke`), family ADR 0022 §G.7
    (no `crates/` change inside a release window).
- Number: 0053. The highest family ADR on any remote branch is 0052 (`git ls-tree --name-only <ref> docs/adr/` over every
  `refs/remotes/origin/*`, 2026-11-04). Check C91: the highest check named in an ADR is C90 (ADR 0052).

## Context

### What is installed per machine today

| App | Windows `installMode` | Released | Hooks |
| --- | --- | --- | --- |
| Pitcrew | `perMachine` (since `a9e1d0736`, 2026-10-22) | 1.0.0, 1.0.1 | registers its LocalSystem service; stops it before an upgrade |
| Pitwall | `perMachine` | 1.0.0 to 1.0.3 | the same, adapted from Pitcrew's |
| Dyno | `currentUser` | 1.0.0 | stops the calling user's agent (`dyno-agent.exe stop`) |
| Spotter | `currentUser` | not yet | none |
| Debrief | default (`currentUser`) | 1.0.0 to 1.1.1 | none |
| Pacenotes, Sighter | none yet | no | none |

Pitcrew and Pitwall already install per machine, but each with its own hand-written hooks: neither stops its app in other
users' sessions, neither guards against a per-user copy, and both still offer "install when the app quits", which on a
per-machine install shows a UAC prompt after the user has quit. `paddock_service::render::nsis_hooks` renders such hooks from a
`ServiceSpec` but knows nothing of the install scope.

### What Tauri does (CLI 2.12.3, `tauri-plugin-updater` 2.14.1, `tauri-plugin-autostart` 2.7.0, `auto-launch` 0.6.0)

Read from the NSIS template inside `@tauri-apps/cli-darwin-arm64` 2.12.3 (its strings) and from the plugin sources in the
Cargo registry, 2026-11-04:

- `perMachine` sets `RequestExecutionLevel admin`, `$INSTDIR = $PROGRAMFILES64\${PRODUCTNAME}` (x64 and arm64), the uninstall
  key in HKLM (`DeleteRegKey HKLM "${UNINSTKEY}"`), shortcuts for all users, and starts the app after install and after an
  update with `nsis_tauri_utils::RunAsUser` (the desktop user's token, not the elevated one).
- `PageReinstall` reads `SHCTX`, which is HKLM in `perMachine`: an existing per-user install (HKCU) is not seen. The template's
  `CheckIfAppIsRunning` asks Restart Manager about **the main binary only** (`$INSTDIR\${MAINBINARYNAME}.exe`), with
  `RmForceShutdown`; sidecars such as an agent are not covered.
- `NSIS_HOOK_PREINSTALL` runs inside `Section Install` right after `SetOutPath $INSTDIR` and **before** `CheckIfAppIsRunning`
  and every `File`: nothing has been copied or written to the registry yet.
- The uninstaller deletes the listed files one by one and then `RMDir "$INSTDIR"` (not recursive), removes
  `HKCU\...\Run\${PRODUCTNAME}` of **the account that runs it** (not when updating), and deletes `$APPDATA\${BUNDLEID}` and
  `$LOCALAPPDATA\${BUNDLEID}` only when "Delete the application data" is ticked.
- The updater on Windows runs the installer with `ShellExecuteW("open")` and the configured mode's arguments plus `/UPDATE`;
  it calls `std::process::exit(0)` only after `ShellExecuteW` succeeded. A declined UAC prompt makes `ShellExecuteW` fail, the
  call returns an error and the app keeps running. `passive` shows the progress window, so UAC can appear; `quiet` cannot elevate.
- `tauri-plugin-autostart` 2.7.0 builds `auto-launch` with no Windows enable mode, so `WindowsEnableMode::Dynamic` (the
  default) applies: an **elevated** process writes `HKLM\...\Run`, which starts the app for every user and which an unelevated
  "off" cannot remove. The plugin has no setter for `CurrentUser`.

### What sessions already share and isolate

Every app runs per session: random pipe names, a session-local single-instance mutex (`tauri-plugin-single-instance`), per-user
data folders and data locks. Several users can therefore run the same per-machine copy at once without seeing each other; what
they share is the files in `$INSTDIR`, which an update or uninstall must replace while other sessions hold them.

## Decision

### 1. The NSIS settings (every per-machine app)

`tauri.windows.conf.json`: `"installMode": "perMachine"` (not `both`: one place, one uninstall key, no mode page), the shared
include and the app's hooks (§2). `tauri.conf.json`: updater `windows.installMode` = `passive`. The install folder must be
under `%ProgramFiles%`: the include's directory check refuses another folder, because programs started from there run as
LocalSystem (Pitcrew, Pitwall, Dyno's helper install) or are elevated by UAC, and a folder the user can write would let any of
that user's programs replace them.

### 2. The shared installer include and its program

`crates/paddock-service/nsis/paddock-per-machine.nsh` (synced into each app's `src-tauri/windows/` by
`paddock_commands::testing::sync_generated`, like the rendered hooks) defines three macros that an app's hooks call. Their
work is done by one small Rust program, `paddock-setup.exe` (a `bin` of `paddock-service`), which the build stages next to the
hooks file and the include embeds with `File "/oname=$PLUGINSDIR\paddock-setup.exe" "${__FILEDIR__}\paddock-setup.exe"`; it
runs from `$PLUGINSDIR`, so it is always the new installer's code and never a file the update is about to replace (WP-S0
confirms that the include can embed it; if not, it is shipped as a resource and the previous version's copy is used, with the
consequence that its first per-machine release cannot stop other sessions).

| Macro | Hook | What it does |
| --- | --- | --- |
| `PADDOCK_GUARD_PER_USER` | `PREINSTALL`, first | `paddock-setup guard --product <name> --main <exe>`: refuses to install while a per-user copy exists (§5). Exit 10 shows the message and quits with error level 2; nothing has been written |
| `PADDOCK_STOP_ALL` | `PREINSTALL`, `PREUNINSTALL` | `paddock-setup stop-all --dir "$INSTDIR" --image <exe>…`: ends every listed program running from `$INSTDIR` in **every** session (§3) |
| `PADDOCK_CLEAN_RUN_VALUES` | `PREUNINSTALL` (not with `/UPDATE`) | removes the app's `Run` value from every loaded user hive when it points into `$INSTDIR` (§6) |

The app's own steps go between them (register or remove its service, Dyno's helper). The include writes a `DetailPrint` line
for each step with the program's exit code and meaning, and `paddock-setup` appends to
`%ProgramData%\Paddock\<identifier>\setup.log` (1 MiB, one rotation): every phase with its start time and duration, every
process it acted on (image file name, pid, start time, session id, outcome), and its exit code. No user names, profile paths
or command lines are logged (ER-27).

### 3. Stopping every session on install, update and uninstall

`paddock_proc::installed::stop_all(dir, images, bounds) -> StopReport`, used by `paddock-setup stop-all`:

1. **Find.** One process snapshot; for each entry whose file name is in `images`, open a handle
   (`PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | PROCESS_TERMINATE`), read the image path from **that handle**
   (`QueryFullProcessImageNameW`) and its start time (`GetProcessTimes`): that is the `ProcessId` (rule 53), held as the
   handle. Another account's process grants an administrator this access only through `SeDebugPrivilege`, so `paddock-setup`
   enables that privilege in its own elevated token for this step and logs whether it could (S0 measures it). The image's
   **parent folder** must equal the canonical `dir` (compared as paths, never as a string prefix:
   `C:\Program Files\Dyno Helper\` starts with `C:\Program Files\Dyno`). Anything else is left alone and logged as skipped.
2. **Ask** (decision O1). Signal the app's stop event in each session that has a match; programs that honour it exit on their
   own. Wait on the held handles, bounded by `bounds.ask` (10 s in all).
3. **End.** `TerminateProcess` on each handle still running, then wait on the handles, bounded by `bounds.end` (5 s each,
   20 s in all). The handle is the identity, so a freed pid can never be hit.
4. **Report.** Exit 0 when nothing listed runs from `dir` any more; exit 11 with the names still running otherwise. The include
   then stops with "<App> could not be closed for every signed-in user. Ask them to quit <App>, then run this installer
   again." Nothing has been copied yet.

These bounds are an installer's, not a UI's (ER-19 is about the app's own jobs): an installer must finish, and when it cannot
it says what to do. Each adopting app states in its adoption that every program in its list survives `TerminateProcess`
without losing more than its last unfinished write (Dyno: SQLite with WAL, ADR 0008).

The caller's own session is not special: the installer may run elevated as another account (an administrator's credentials
typed by a standard user), so "the user who started the installer" is not knowable from its token. The template's Restart
Manager check stays as a second line for the main binary.

### 4. Updates

`paddock-updater` gets an `InstallScope` (`PerUser`, `PerMachine`) declared by the app in its composition (ADR 0037); `PerMachine`
on Windows means:

- **One UAC prompt per update**, from the installer itself. The Install button says "Install (Windows will ask for permission)".
- **No install when the app quits.** `install_on_quit` answers `NotAvailable` with "This copy is installed for all users, so
  updates are installed when you click Install"; a saved "on" is ignored, and the setting is hidden. A prompt after Quit (or at
  sign-out, when nobody answers it) is never right (decision O4).
- **Accounts that are not administrators** (not a member of Administrators, even with a filtered token) see "An administrator
  must install this update. Windows will ask for an administrator's name and password." The button stays (decision O3).
- **Declined prompt:** `ShellExecuteW` fails and the app keeps running; the state is "Not installed: Windows permission was
  declined", with Install again. Logged with the error code.
- **`quiet` is forbidden** for a per-machine app (it cannot elevate): check C91.
- After the install, the template starts the app again as the desktop user (`RunAsUser`), in the updating user's session only.
  Other sessions' copies were ended by §3 and are not restarted (decision O2); each user's next start or login starts the new
  version.

### 5. The per-user guard (no migration)

There is no migration. A per-user copy is uninstalled by hand first. `paddock-setup guard` is a pure decision
(`paddock_service::installed::per_user_copies` over a `InstallFacts` port, unit-tested with fakes) over:

- the account running the installer: `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<productName>` (Tauri's
  `UNINSTKEY`) and `%LOCALAPPDATA%\<productName>\<main>.exe`;
- every **loaded** user hive (`HKU\<SID>`, the signed-in users, which covers a standard user who typed an administrator's
  credentials): the same key, and the same file under that profile's `ProfileImagePath`.

Any hit stops the installer before it changes anything: "Uninstall the older <App> first (Settings › Apps), then run this
installer again." When the copy belongs to another signed-in account the message adds "It is installed for <account>." A
per-user copy of an account that is not signed in is reported in the log and does not block (decision O5). The guard never
changes, moves or deletes anything.

**An older per-user release meets the new feed.** Its updater downloads the per-machine installer and runs it; UAC appears;
the guard stops with the message above; the old copy stays installed and usable, with its data. Each app's release notes for
its first per-machine version say so, and say how to uninstall without losing data (Dyno's: §1 of its amendment).

### 6. Start at login, always per user

`paddock_service::login::UserRunEntry`, an `Autostart` that writes the app's own value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the `Run` backend `paddock-service` already has for user agents, and its
`StartupApproved` handling), with the running executable and the app's start arguments. It:

- never writes HKLM;
- **does nothing while the process is elevated** (`TokenElevation`): an elevated process may be another account (typed
  credentials), so its HKCU is not the desktop user's. It logs "start at login is set from a normal start" and leaves the
  first-run marker unwritten, so the next unelevated start applies it;
- replaces `tauri-plugin-autostart` on Windows in every app (the plugin cannot select `CurrentUser`, §Context). Linux keeps the
  plugin; macOS keeps `SMAppService.mainApp`.

Each user's first unelevated start applies the app's default (Dyno: on, from `feat/dyno-launch-at-login-default`) through it;
the marker is per user (the app data folder), so a per-machine install gives every user their own default and switch.

**Uninstall and other users' entries.** The template removes the uninstalling account's value. `PADDOCK_CLEAN_RUN_VALUES`
removes the value from every loaded hive when its command points into `$INSTDIR`. Accounts that are not signed in keep a value
that names a missing program; Windows skips it at sign-in and Task Manager lists it as an entry without a program (decision O6).
Other users' data folders are never touched; "Delete the application data" applies to the uninstalling account only.

### 7. Failure modes

| Failure | What happens |
| --- | --- |
| A per-user copy for this account or a signed-in one | guard message; exit 2; nothing written (§5) |
| A per-user copy of a signed-out account | logged; install continues; that account's old copy keeps running, and its updater meets the guard |
| An older per-user release's updater runs the new installer | UAC, then the guard message; the old copy is unchanged |
| UAC declined (install or update) | nothing changes; the app keeps running; "Windows permission was declined" with Install again |
| A standard user without administrator credentials | the update cannot be installed; the status says an administrator must install it |
| A listed program in another session does not end within the bound | exit 11; message names the app; nothing copied; log lists image, pid, start time, session |
| A process with the same file name outside `$INSTDIR` (another copy, the helper's protected copy) | not touched; logged as skipped |
| A pid reused between snapshot and end | impossible: the handle opened in step 1 is the identity |
| Install folder outside `%ProgramFiles%` chosen on the directory page | refused by the directory check with the reason |
| `quiet` configured for a per-machine app | C91 fails the build |
| The app started elevated (Run as administrator) | no `Run` write, logged; HKLM never written |
| Another session's app ended by an update | not restarted; starts at that user's next login or open (O2) |
| Uninstall with signed-out users who had start at login | their `Run` value stays and names a missing program (O6) |
| `paddock-setup.exe` missing from the installer | C91 and the ADR 0039 artifact gate fail the release build |

### 8. Checks

- **C91 (per-machine installer contract)**, over every app with `installMode: perMachine`: updater mode `passive`; the hooks call
  the three macros in the right hooks; the synced include is current; the app declares `InstallScope::PerMachine`; no
  `tauri-plugin-autostart` on the Windows target.
- **ADR 0039 artifact gate:** the built NSIS installer requests administrator level, contains `paddock-setup.exe` and the
  include's version string.
- **Unit tests:** the guard decision (each source, loaded and unloaded hives, nothing written); `stop_all`'s folder comparison
  (the `Dyno Helper` prefix case), bounds and report over a fake process table, plus a Windows test with real children of
  `paddock_proc::testchild` in a temporary folder (ER-14: only processes the test started); `UserRunEntry` over the fake `System`
  (HKCU only, nothing when elevated); the updater's scope rules.

### 9. The reusable VM suite

`scripts/vm-smoke` gains a Windows per-machine suite, parameterised by the app's `paddock.ci.json` (product name, images,
service, helper hooks), run in the local Windows VM under the ADR 0040 GUI session and two accounts (`paddock-admin`,
`paddock-test` standard). Local only, never CI (ER-14). The suite belongs to the `vm-smoke` harness and lands from its home
(ADR 0042).

| # | Case | Passes when |
| --- | --- | --- |
| PM-1 | Fresh install as an administrator | files in `%ProgramFiles%\<App>`; HKLM uninstall key; all-users Start menu shortcut; the app started by the installer is not elevated and runs as the desktop user; the first-run default wrote HKCU `Run`, HKLM `Run` has no value |
| PM-2 | Fresh install as a standard user with an administrator's credentials | as PM-1, and the app, its `Run` value and its data belong to the standard user; the administrator's hive is unchanged |
| PM-3 | Guard: a per-user copy for this account, for a signed-in other account, and for a signed-out one | the first two stop with the message and leave no HKLM key, no folder and no file; the third is logged and installs |
| PM-4 | The last per-user release updates from the feed | UAC, then the guard message; the old copy starts again and its data is intact |
| PM-5 | Uninstall the per-user release by hand, then install per machine | the data folder survives the uninstall (with "Delete the application data" left unticked); the per-machine app reads it |
| PM-6 | Two users with Fast User Switching, one updates from the app | one UAC prompt; the other session's programs end (log: pid, start time, session); the files are replaced; the updating user's app restarts unelevated; switching back, the other user starts the new version from the Start menu with its data; no orphan process |
| PM-7 | A standard user updates | the "administrator must install" text; with credentials it installs; Cancel leaves the app running and says the prompt was declined |
| PM-8 | Install on quit | not offered |
| PM-9 | Uninstall with two signed-in users and one signed-out user | every session's programs end; HKLM key and folder gone; both signed-in users' `Run` values gone; the signed-out user's value stays (O6); every user's data stays |
| PM-10 | Reboot with start at login on for both users | at each sign-in: one app, one of each sidecar, the tray icon, nothing elevated, no HKLM `Run` value |
| PM-11 | The app started with Run as administrator | no `Run` write, the log line; the next normal start applies the switch |

An app adds its own cases (Dyno: the helper, in its amendment).

### 10. Parallelism and cost (rule 51)

`stop_all` reads the process table once per install (a few hundred entries) and waits on handles, never by polling (ADR
0024); the ends run on one thread because there are at most a few processes per session. The guard reads a handful of
registry keys and files. Neither adds more than a second to an install when nothing is running.

## Work packages

Agent-days for one agent. **Shared** marks where the code lives. The shared `crates/` packages are additive (new module,
new `bin`, a new enum with a `PerUser` default), so they do not force an app change (rule 43) and can land with Dyno's adoption
in one batch; they still follow family ADR 0022 §G.7 (not inside another app's release window).

| WP | Package | Shared | Owned paths | Depends on | Days |
| --- | --- | --- | --- | --- | --- |
| S0 | VM spikes: embedding `paddock-setup.exe` via `${__FILEDIR__}`; `Quit` and error level from `PREINSTALL` in `passive` and `/UPDATE`; a per-session stop event the elevated installer can signal (O1); opening and ending another account's process with `SeDebugPrivilege` enabled; whether the guard's `MessageBox` shows in `passive`; Restart Manager across sessions; that a 1.0-style per-user uninstall keeps `%LOCALAPPDATA%\<productName>` data | – | `docs/research/` note only | – | 1–1.5 |
| S1 | `paddock_proc::installed::stop_all` (find by handle and folder, ask, end, report) with unit and Windows tests | crates/ | `crates/paddock-proc/src/installed.rs` (+ one `lib.rs` line) | S0 | 1.5–2 |
| S2 | `paddock-service`: `installed::per_user_copies` (pure, `InstallFacts` port), `bin/paddock-setup` (`guard`, `stop-all`, `clean-run-values`, the log file), `login::UserRunEntry` | crates/ | `crates/paddock-service/src/{installed.rs,login.rs}`, `src/bin/paddock-setup.rs`, its `Cargo.toml` `[[bin]]` | S1 | 2–3 |
| S3 | `nsis/paddock-per-machine.nsh` (the three macros, directory check, messages), `render::nsis_hooks` for `PerMachine`, sync test | crates/ | `crates/paddock-service/nsis/`, `src/render.rs` | S2 | 1–1.5 |
| S4 | `paddock-updater` `InstallScope`, install-on-quit refused, admin-needed and declined states; the updater panel text | crates/, ui/ | `crates/paddock-updater/src/{state.rs,updater.rs,tauri.rs}`, the updater component in `ui/projects/paddock-ui` | – | 1.5–2 |
| S5 | Conformance C91 and the artifact-gate entries | conformance/ | `conformance/src/checks/C91-per-machine.mjs`, `conformance/release/` | S3, S4 | 0.5–1 |
| S6 | The Windows per-machine suite in `scripts/vm-smoke` (PM-1 to PM-11), its `paddock.ci.json` fields, the testing-manual page | scripts/ (vm-smoke home) | `scripts/vm-smoke/` | S3 | 2–3 |
| S7 | New apps: `scripts/new-app.mjs` writes the per-machine config, the hooks and the scope | new-app | `scripts/new-app.mjs`, `templates/` | S3, S4 | 0.5 |
| D | Dyno's adoption (its amendment: config, hooks, scope, `UserRunEntry`, helper steps, release notes, its VM cases) | Dyno | see the amendment | S2–S4 | 4–6 |

**Shared: 10 to 14.5 agent-days; Dyno: 4 to 6; in all 14 to 20.5.** Order: S0 → S1 → S2 → S3 → D → VM runs, with S4, S5, S6
and S7 beside them. With three agents the critical path is about 7 to 10 working days, inside the 1 to 1.5 weeks the user
accepted.

### Adoption by the other apps (later, each in its own release line)

| App | Today | Adoption | Days |
| --- | --- | --- | --- |
| Pitcrew | `perMachine`, own hooks | hooks onto the include (stop `pitcrew-app.exe` and its service program in every session; the service's own `uninstall` stays), `InstallScope::PerMachine` (install-on-quit goes), `UserRunEntry`, PM suite. The guard never fires (no per-user release) | 1–1.5 |
| Pitwall | `perMachine`, own hooks | the same as Pitcrew | 1–1.5 |
| Spotter | `currentUser`, unreleased | per machine from its first release if S1 to S4 land before its tag (decision O7); no per-user users exist, so the guard is only a safety net | 1–1.5 |
| Debrief | `currentUser`, 1.0.0 to 1.1.1 | in its next feature release (1.2), with the guard and release notes as for Dyno | 1.5–2 |
| Pacenotes, Sighter | none | per machine from the first release, through the template | in their packaging WPs |
| shell-demo, workbench-demo | demo releases | only if they ship a Windows installer | 0.5 |

## Consequences

- Every Windows app installs once per PC, for every user, in a folder only administrators can change; Windows starts each
  user's copy from that user's own `Run` value.
- Every update asks for administrator permission once, and standard users need an administrator. Nothing installs on quit.
- An update or uninstall closes the app for everyone signed in, and says so in the log; the other users are not restarted.
- A user with an older per-user copy must uninstall it first; the installer says so and changes nothing.
- One include, one program and one `Autostart` serve every app, so a fix to how sessions are stopped or how login entries are
  written lands once (rule 55). Pitcrew and Pitwall's hand-written hooks go when they adopt.

## Decisions still open (recommended defaults)

| # | Question | Recommended default |
| --- | --- | --- |
| O1 | Ask programs to quit before ending them (a per-session stop event that the app shell and the agents wait on), or end them at once | **Ask first** if S0 shows the elevated installer can signal another session's event; otherwise end only (the programs tolerate it) |
| O2 | Other sessions after an update: not restarted, or a note at their next start ("<App> was updated to X while you were away") | **Not restarted, no note** |
| O3 | A standard user's update: keep the Install button (Windows asks for an administrator's credentials) or hide it | **Keep it**, with the "administrator must install" text |
| O4 | Install on quit on per-machine Windows: off entirely, or only on an explicit Quit from the tray | **Off entirely** |
| O5 | The guard and a per-user copy of a signed-out account: log only, or block too | **Log only** (blocking would make one absent account stop everyone; that account's own updater meets the guard) |
| O6 | Uninstall and signed-out users' `Run` values: leave them (Windows skips a missing program), or remove them at their next sign-in through Active Setup | **Leave them**; Active Setup only if users report the leftover entry |
| O7 | Spotter: per machine from 1.0 if the shared parts land before its tag, otherwise in 1.1 | **Yes** |
| O8 | One update feed for the last per-user release and the first per-machine one (the guard stops the update with its message), or a separate feed so per-user copies are never offered it | **One feed**: per-user users are told what to do |
