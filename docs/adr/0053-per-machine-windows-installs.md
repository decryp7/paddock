# 53. Per-machine Windows installs: one shared mechanism, Dyno first

- Status: Proposed (2026-11-04), revision 3. The user decided the direction on 2026-11-04: Dyno 1.1.0 on Windows installs per
  machine, to `C:\Program Files\Dyno`, for all users; start at login is always written per user (HKCU), even when the app runs
  elevated; the other apps follow on the same mechanism (rule 42 / ER-22); **no migration**: a per-user copy is uninstalled by
  hand, and the per-machine installer refuses a fresh install while one is there ("no backward compatibility before v1").
- Revision 2 (2026-11-04): answers the independent review of `86e1ac62a` ("not yet"). P1-1: a manual upgrade's old uninstaller
  acts as an update (§2.4). P1-2: §4 follows `paddock-updater`'s real exit-hook flow. P1-3: the guard runs on a fresh install
  only and blocks only for the installer's account and the desktop user, on the uninstall key (§5). P1-4: the elevated side
  copies the download into an administrator-only folder and verifies its signature again before running it (§4.2, the trust
  model). P2 1 to 10: Restart Manager first (§3), what exists before the guard and the clean-up on abort (§2.3), the directory
  check in `PREINSTALL` (§2.2), the stop list derived from the bundle config (§2.1), "Delete the application data" (§6.3),
  `TokenElevationTypeFull` (§6.1), a leftover per-user copy at the app's start (§6.2), HKLM `Run` values (§6.3), the missing
  work packages and a new estimate, and the hive scan (§5). The user's decisions on the open questions of revision 1 are under
  "Decisions (the user, 2026-11-04)"; the full design ships, not a slice.
- Revision 3 (2026-11-04): answers the re-review of `ca3b6365e`. P1: nothing elevated is staged or logged under
  `%ProgramData%`, which a standard user can pre-create; the update is staged and `setup.log` written in `$INSTDIR\updates\`
  (§2.2's chain), and the verified copy stays open with read-only sharing until `CreateProcessW` returns (§2.5, §4.2). P2: the
  `QUERY_LIMITED` open without `SeDebugPrivilege` measured in S0 (§3); the reinstall marker compares folders canonically and
  without case, the HKLM uninstall key text corrected, a cancel after the in-place uninstall (§2.4, §7, PM-12b), same-version
  "Uninstall"; `apply-update` takes a regular local file only, opened for identification, says what a hand-off timeout does,
  carries a version-resource description, and "What stays" names the split-token `%TEMP%` point (§4.2); the abort restart only
  for the desktop user's own hit and no "stopped" message while the installer still runs (§4.1, §4.3, §5); H1's order (Dyno
  amendment); the last `RmGetList` is in `PREINSTALL`.
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
    [0031](0031-patience-not-wall-clocks.md) (bounded waits), ADR [0037](0037-app-composition-contract.md) (composition),
    ADR [0039](0039-release-gates-inspect-the-artifacts.md) (release gates inspect the artifacts), ADR
    [0040](0040-windows-gui-test-session.md) (Windows GUI test session), ADR
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

Pitcrew and Pitwall already install per machine, each with its own hand-written hooks: neither stops its app in other users'
sessions, neither guards against a per-user copy, and both still offer "install when the app quits", which on a per-machine
install shows a UAC prompt after the user has quit. `paddock_service::render::nsis_hooks` renders such hooks from a
`ServiceSpec` but knows nothing of the install scope.

### What Tauri and `paddock-updater` do

Read from the NSIS template inside `@tauri-apps/cli-darwin-arm64` 2.12.3 (its strings), the plugin sources in the Cargo
registry (`tauri-plugin-updater` 2.14.1, `tauri-plugin-autostart` 2.7.0, `auto-launch` 0.6.0) and `crates/paddock-updater` at
`origin/dyno/wp-int` `74803654f`, 2026-11-04:

- **Install mode.** `perMachine` sets `RequestExecutionLevel admin`, `$INSTDIR = $PROGRAMFILES64\${PRODUCTNAME}` (x64 and
  arm64), the uninstall key `UNINSTKEY` = `Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}` in HKLM, all-users
  shortcuts, and starts the app after install with `nsis_tauri_utils::RunAsUser` (the desktop user's token) when `/R` is given in
  passive or silent mode (`.onInstSuccess`).
- **Order inside `Section Install`:** `SetOutPath $INSTDIR` (creates the folder), then `NSIS_HOOK_PREINSTALL`, then
  `CheckIfAppIsRunning` (Restart Manager on **the main binary only**, `RmForceShutdown`), then the first `File`. An earlier
  section may already have installed the WebView2 runtime.
- **Reinstall page.** `PageReinstall` reads `SHCTX` (HKLM here): a per-user install is not seen. When a per-machine install is
  there and the user keeps the default choice (and in passive mode on the same version), `PageLeaveReinstall` runs the old
  uninstaller **in place** (`"<UninstallString>" _?=<old dir>`), adding `/UPDATE` only when the new installer itself has
  `/UPDATE`. A manual upgrade therefore runs the old uninstaller as a full uninstall.
- **Uninstaller.** `un.onInit` sets `$UpdateMode` from `/UPDATE`; `PREUNINSTALL` runs first in `Section Uninstall`. Then it deletes
  the listed files one by one and `RMDir "$INSTDIR"` (not recursive); it deletes the HKLM uninstall key **whatever
  `$UpdateMode` says** (`DeleteRegKey HKLM "${UNINSTKEY}"` for `perMachine`); unless `$UpdateMode = 1` it removes the shortcuts
  and `HKCU\...\Run\${PRODUCTNAME}`; with "Delete the application data" ticked and not updating, it switches to
  `SetShellVarContext current` and deletes `$APPDATA\${BUNDLEID}` and `$LOCALAPPDATA\${BUNDLEID}` **of the account the uninstaller
  runs as**.
- **`tauri-plugin-updater`** runs the installer with `ShellExecuteW("open")` and the mode's arguments (`/P` for passive) plus
  `/UPDATE`, and `/R` when the app asked for a restart; it calls `std::process::exit(0)` only after `ShellExecuteW` succeeded.
  A declined UAC prompt makes `ShellExecuteW` fail, and the call returns an error. `quiet` (`/S`) cannot elevate.
- **`paddock-updater`'s flow** (`updater.rs`, `tauri.rs`): Install marks `OnExit::InstallAndRelaunch` and quits the app
  (`updates_restart`); at `RunEvent::Exit`, `install_at_exit` runs the services' `before_install`, then the `Installer`
  (`tauri-plugin-updater`, which verifies the minisign signature again and runs the file from the user's temporary folder). On
  an error it records `failed_install` and returns `ExitAction::Failed { relaunch }`; `relaunch_after` starts the old app again,
  which shows the failure as its first phase. "Install when the app quits" (`OnExit::Install`) does the same without relaunch.
- **`tauri-plugin-autostart` 2.7.0** builds `auto-launch` with no Windows enable mode, so `WindowsEnableMode::Dynamic` applies:
  an elevated process writes `HKLM\...\Run`, which starts the app for every user and which an unelevated "off" cannot remove.
  The plugin has no setter for `CurrentUser`.

### What sessions already share and isolate

Every app runs per session: random pipe names, a session-local single-instance mutex (`tauri-plugin-single-instance`), per-user
data folders and data locks. Several users can run the same per-machine copy at once; what they share is the files in
`$INSTDIR`, which an update or uninstall must replace while other sessions hold them.

## Decision

### 1. The NSIS settings (every per-machine app)

`tauri.windows.conf.json`: `"installMode": "perMachine"` (not `both`: one place, one uninstall key, no mode page), the shared
include and the app's hooks (§2). `tauri.conf.json`: updater `windows.installMode` = `passive`; `quiet` is refused by C91. The
install folder must be under `%ProgramFiles%` (§2.2): programs started from there run as LocalSystem (Pitcrew, Pitwall, Dyno's
helper install) or are elevated by UAC (§4.2), and a folder the user can write would let any of that user's programs replace
them.

### 2. The shared installer include and `paddock-setup.exe`

A new crate `crates/paddock-setup` (a library and one `bin`) does the installer's work; `paddock-setup.exe` is built **per app**
in the app's Windows bundle job with the app's minisign public key compiled in (`PADDOCK_SETUP_UPDATE_KEY`, the same key as
`plugins.updater.pubkey`; C91 compares them) and staged next to the hooks file. The include
`crates/paddock-setup/nsis/paddock-per-machine.nsh` (synced into each app's `src-tauri/windows/` by
`paddock_commands::testing::sync_generated`) embeds it with `File "/oname=$PLUGINSDIR\paddock-setup.exe"
"${__FILEDIR__}\paddock-setup.exe"`, so the installer always runs its own copy, and also installs it into `$INSTDIR` (the
updater's elevated hand-off, §4.2, and the uninstaller run it from there).

| Macro | Hook | What it does |
| --- | --- | --- |
| `PADDOCK_CHECK_DIR` | `PREINSTALL`, first | `paddock-setup check-dir --dir "$INSTDIR"` (§2.2) |
| `PADDOCK_GUARD_PER_USER` | `PREINSTALL`, fresh install only | `paddock-setup guard` (§5) |
| `PADDOCK_STOP_ALL` | `PREINSTALL`, `PREUNINSTALL` | `paddock-setup stop-all --dir "$INSTDIR" --images ${PADDOCK_STOP_IMAGES}` (§3), then, still in `PREINSTALL` (or `PREUNINSTALL`), a last `RmGetList` over the same files |
| `PADDOCK_REINSTALL_AS_UPDATE` | `PREUNINSTALL`, first | an in-place run is a reinstall: `StrCpy $UpdateMode 1` (§2.4) |
| `PADDOCK_CLEAN_RUN_VALUES` | `POSTINSTALL`; `PREUNINSTALL` unless `$UpdateMode = 1` | §6.3 |
| `PADDOCK_ABORT` | every refusal | the message, the clean-up of §2.3, the restart of §4.3 or §5, `SetErrorLevel` and `Quit` |
| `PADDOCK_INSTALLING` | `PREINSTALL`, before `PADDOCK_CHECK_DIR`; `PREUNINSTALL` | creates and holds `Global\<identifier>.setup`, a named mutex, until the process exits (§4.1 step 4) |

The app's own steps go between them (register or remove its service, Dyno's helper). Each step writes a `DetailPrint` line
with the program's exit code and its meaning, and `paddock-setup` appends to `$INSTDIR\updates\setup.log` (1 MiB, one
rotation; §2.5) once `check-dir` has passed (a `check-dir` refusal is in the installer's details only, because the folder is
the thing refused): each phase with its start time and duration, every process acted on (image file name, pid, start time, session id, the method, the outcome),
and the exit code. No user names, profile paths or command lines are logged (ER-27).

#### 2.1 The stop list comes from the bundle config

`paddock_service::render` writes `paddock-per-machine.generated.nsh` with `!define PADDOCK_STOP_IMAGES` from `mainBinaryName`
plus the file name of every `bundle.externalBin` entry (with Tauri's target-triple suffix removed, as installed), minus the
app's declared exclusions, each with a reason (Dyno: `dyno-helper.exe`, its amendment). It is synced like the include, so a
new sidecar is in the list without anyone remembering it; C91 compares the generated list with the config and the exclusions.
`paddock-setup.exe` itself is always in the list (a hand-off that is still exiting); `stop-all` never ends its own process (its
own pid and start time), which matters in `PREUNINSTALL`, where it runs from `$INSTDIR`.

#### 2.2 The directory check, twice

The directory page cannot be relied on: it is skipped in passive mode and on `/UPDATE`, and `/D=` sets the folder from the
command line. `check-dir` runs first in `PREINSTALL` (and the include's `.onVerifyInstDir` gives the same answer on the page): the
canonical `$INSTDIR` must be a direct child of `%ProgramFiles%` (or `%ProgramFiles(x86)%` for a 32-bit build) by path
components, no component may be a reparse point, and its owner must be SYSTEM, Administrators or TrustedInstaller when it
exists. Otherwise `PADDOCK_ABORT` with "Install <App> in Program Files" and the folder named.

#### 2.3 What exists before a refusal, and what a refusal removes

Before the first macro runs: `$INSTDIR` exists (the template's `SetOutPath`; empty on a fresh install), `$PLUGINSDIR` holds the
installer's temporary files, and the WebView2 runtime may have been installed by the template's earlier section. A refusal
after `check-dir` has written `$INSTDIR\updates\setup.log`. Nothing else is written: no other file in `$INSTDIR`, no registry
value, no shortcut. On a refusal during a fresh install (no HKLM uninstall key), `PADDOCK_ABORT` runs `RMDir "$INSTDIR"` (not
recursive) before `Quit`: after a `check-dir` refusal the folder is empty and goes; after a later refusal it holds only
`updates\setup.log`, which stays on purpose (the evidence, readable by users; the message names it) and is reused by the next
run. The WebView2 runtime stays (it is a shared Windows component).

#### 2.4 A manual upgrade's old uninstaller acts as an update (the reinstall marker)

A user who runs a newer installer by hand reaches `PageLeaveReinstall`, which runs the old uninstaller in place without
`/UPDATE`. Without a guard that uninstall would remove the shortcuts, the caller's `Run` value, the other users' `Run` values
(§6.3) and an app's service or helper. The marker is the in-place run itself: an NSIS uninstaller started without `_?=` copies
itself to the temporary folder and runs from there, and the template starts the old uninstaller with `_?=` only from
`PageLeaveReinstall`. So `PADDOCK_REINSTALL_AS_UPDATE`, first in `PREUNINSTALL`, asks `"$INSTDIR\paddock-setup.exe" same-folder
--a "$EXEDIR" --b "$INSTDIR"`, which compares the two **canonically and without case**: each folder opened (without following
a final reparse point) and named by `GetFinalPathNameByHandleW` (long names, so 8.3 short names and a trailing backslash do not
matter), then `CompareStringOrdinal` with `ignoreCase`; a folder that cannot be opened is "not the same". When they are the
same it is a reinstall: it sets `$UpdateMode 1` (which the template checks after the hook) and logs "reinstall: treated as an
update". An uninstall from Settings › Apps or from `uninstall.exe` started by hand runs from the temporary copy and is a real
uninstall.

What "as an update" keeps and what it does not: the shortcuts, every user's `Run` value, an app's service or helper and its
data stay. The HKLM uninstall key is deleted anyway (the template deletes it whatever `$UpdateMode` says), and the program files
go; the new installer writes both again right after. On the same version, the reinstall page's "Uninstall" choice is the same
in-place run and therefore also acts as an update: the template installs again after it in both choices, so nothing is lost.

**A cancel between the two halves.** If the new installer is cancelled, or fails, after the old uninstaller ran in place, the
PC holds a half-removed app: no program files and no HKLM uninstall key, but shortcuts that point at nothing, `Run` values that
name a missing program, and (an app with a service or helper) a service whose program folder is gone. Running the installer
again is a fresh install (no HKLM key, so the guard runs and finds no per-user copy) and restores the files; the shortcuts and
`Run` values then work again. Dyno's helper runs from its protected copy and keeps running; its orphan watch removes it if the
app is still missing after one hour (Dyno's amendment), after which it is turned on again in Settings. PM-12b tests it. **Every
per-machine release's uninstaller must carry this macro, starting with the first** (Dyno 1.1.0), because the next version's
installer runs it. S0 confirms that `$EXEDIR` tells the two apart and that the template reads `$UpdateMode` after the hook; if
either fails, a marker written by the new installer is not an option (the template owns `.onInit` and the reinstall page,
which run before any hook), so the fallback is `PREUNINSTALL` asking `paddock-setup is-reinstall` whether its parent process is
an installer of the same product (the parent's image and version resource, read through a held handle).

#### 2.5 Where elevated code stages and logs: `$INSTDIR\updates\`

Nothing elevated is staged, logged or read under `%ProgramData%`. A standard user can create `%ProgramData%\Paddock` (the default
`%ProgramData%` ACL lets users create folders and owns them as the creator), put a junction or hard-link target in it, and so
steer an elevated write or a staged file. `$INSTDIR\updates\` cannot be pre-created by a standard user: `%ProgramFiles%` lets
only administrators and TrustedInstaller create children, and §2.2 checked that `$INSTDIR` is a direct child, with no reparse
point, owned by SYSTEM, Administrators or TrustedInstaller. `paddock-setup` makes `updates\` (or checks an existing one: same
owner rule, no reparse point, else renamed aside and made again) with an explicit descriptor: SYSTEM and Administrators full,
Users read, no inheritance, through `paddock_service::privileged`'s protected-folder rule (rule 55). It holds `setup.log` and the
staged update (§4.2). The uninstaller removes it with the program files (not on a reinstall, which keeps the log).

### 3. Stopping every session on install, update and uninstall

`paddock_proc::installed::stop_all(dir, images, bounds) -> StopReport`, used by `paddock-setup stop-all`:

1. **Restart Manager first.** One session; `RmRegisterResources` with the full path of every listed image in `dir`;
   `RmGetList` names every process in any session that uses them, each as `RM_UNIQUE_PROCESS` (pid plus start time, which is the
   identity of rule 53) with its session id. `RmShutdown(RmForceShutdown)` asks them to close (GUI programs get
   `WM_QUERYENDSESSION`/`WM_CLOSE`, console programs a control event) and ends those that do not, within `bounds.rm` (20 s in
   all). No `RmRestart`: other sessions' programs are not restarted (decision O2). `RmGetList` again: an empty list is done.
2. **Fallback: by handle, with `SeDebugPrivilege`.** Only for what Restart Manager could not list or end (it reports
   `RmStatusUnknown`, a process it could not shut down, or the session failed). One process snapshot; for each entry whose
   file name is in `images`: open a handle (`PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`), read the image path and start
   time **from that handle**. S0 measures whether an elevated administrator can make that open on another account's process
   without `SeDebugPrivilege`; if not, the privilege is enabled around the open too, only inside this fallback loop, and
   dropped after each entry; the image's parent folder must equal the canonical `dir` (compared as paths, never as a string
   prefix: `C:\Program Files\Dyno Helper\` starts with `C:\Program Files\Dyno`), otherwise it is left alone and logged as
   skipped. Only then is `SeDebugPrivilege` enabled in `paddock-setup`'s own token (it is present but disabled in an elevated
   administrator's token), a second handle with `PROCESS_TERMINATE` is opened and checked to be the same process (same start
   time), `TerminateProcess` runs, and the privilege is disabled again at once; the wait on the held handles is bounded by
   `bounds.end` (5 s each, 20 s in all). If the privilege cannot be enabled (a policy removed it), that is logged with the
   error and the step reports what is still running.
3. **Report.** Exit 0 when nothing listed runs from `dir` (a last `RmGetList`, run by the include at the end of `PREINSTALL`, so
   before the template's `CheckIfAppIsRunning` and its first `File`); exit 11 with the image names still running otherwise, and `PADDOCK_ABORT` says "<App> could not be closed
   for every signed-in user. Ask them to quit <App>, then run this installer again."

These bounds are an installer's, not a UI's (ER-19 is about the app's own jobs): an installer must finish, and when it cannot
it says what to do. Each adopting app states in its adoption that every program in its list survives a forced end without
losing more than its last unfinished write (Dyno: SQLite with WAL, ADR 0008).

The caller's own session is not special: the installer may run elevated as another account (an administrator's credentials
typed by a standard user), so "the user who started the installer" is not knowable from its token. Other sessions' programs
are **ended and not restarted** (decision O1/O2): their tray icon goes away until that user starts the app again or signs in
again. Each app's release notes and user guide say so.

### 4. Updates

`paddock-updater` gets an `InstallScope` (`PerUser`, `PerMachine`) declared by the app in its composition (ADR 0037).
`PerMachine` on Windows changes the install step; checking and downloading stay as they are (unelevated, in the user's
account, the signature checked after the download).

#### 4.1 The flow

1. **Install** (`updates_restart`) marks `InstallAndRelaunch` and quits the app. "Install when the app quits" does not exist for
   `PerMachine` (decision O4): `install_on_quit` answers `NotAvailable` with "This copy is installed for all users, so updates
   are installed when you click Install", a saved "on" is ignored, and the setting is hidden. A prompt after Quit, or at
   sign-out when nobody answers it, is never right.
2. **Exit hook.** `install_at_exit` runs `before_install`, writes a hand-off record (`handoff: { version, at }`, in the
   updater's records) and calls the `Installer`. For `PerMachine` the installer is `ElevatedHandoff`, not
   `tauri-plugin-updater`: `ShellExecuteExW` with the verb `runas` on `$INSTDIR\paddock-setup.exe apply-update --file
   <download> --signature <signature file> --version <v> --restart` (`$INSTDIR` from the running executable's folder, which
   §2.2 made a Program Files folder). The UAC prompt names `paddock-setup.exe` in `C:\Program Files\<App>` (unsigned: "Unknown
   publisher" until the family has a signing identity; the program name shown is the version resource's description, "<App>
   update"). It waits for `paddock-setup.exe`'s exit code (it only copies, checks and starts the installer: seconds), bounded
   by the hand-off's patience (ADR 0031).
3. **Outcomes in the exit hook.**
   - UAC declined: `ShellExecuteExW` fails with `ERROR_CANCELLED` (1223). `InstallFailed("Windows permission was declined")`;
     `failed_install` recorded; `ExitAction::Failed { relaunch: true }`: the old version starts again and shows "Not
     installed: Windows permission was declined" with Install again. Logged with the error code.
   - `apply-update` refused (signature, version, folder; its exit code maps to a sentence): the same Failed path with that
     sentence, for example "The downloaded update did not pass the signature check. Download it again."
   - Handed off (exit 0): `ExitAction::Installed { relaunch: false }`; the app exits and the installer restarts it (§4.3).
   - The wait runs out (the hand-off is still working, for example on a slow disk): the outcome is unknown, so the app does not
     start itself again (that would race the installer's stop): it logs "hand-off still running", keeps the hand-off record and
     exits as in the handed-off case. Whatever happens next is settled by step 4 at the next start: the installer's own
     restart, or the user's.
4. **Start after the update.** The new version clears the hand-off record. A start that finds a hand-off record for a version
   newer than itself first checks whether an installer of this app is still running: `paddock-setup apply-update` and the
   include (`PADDOCK_INSTALLING`) hold the named mutex `Global\<identifier>.setup` while they run (a standard user cannot create
   `Global\` objects, which needs `SeCreateGlobalPrivilege`, so the name cannot be squatted to fake a running install). While it
   exists the app shows "An update is being installed" and waits on it (a wait on the mutex, no polling); once it is gone, a
   record still naming a newer version means the installer did not finish: Failed with "The installer stopped before it
   finished. The reason is in <install folder>\updates\setup.log" and Install again, and the record is cleared. The old version
   started by `PADDOCK_ABORT` (§4.3) therefore starts while the installer still holds the mutex and shows the failure only after
   it has quit.
5. **Accounts that are not administrators** (not a member of Administrators, even with a filtered token) see "An administrator
   must install this update. Windows will ask for an administrator's name and password." The button stays (decision O3).

#### 4.2 The trust model of `apply-update`

- **Trust anchor:** the minisign public key compiled into `paddock-setup.exe`, which sits in `$INSTDIR`, a Program Files folder
  only administrators and TrustedInstaller can change (§2.2). An administrator who approves the prompt approves that program.
- **Untrusted:** everything the unelevated side hands over: the download in the user's temporary folder (any program of the
  user, or of a standard user whose prompt an administrator approves, can replace it at any time until it is copied), the
  signature file, the version and every argument.
- **Steps, all in the elevated process,** holding `Global\<identifier>.setup` (§4.1 step 4):
  1. Make or check `$INSTDIR\updates\` (§2.5); `$INSTDIR` is the folder of the running `paddock-setup.exe`, checked again by
     `check-dir`'s rule.
  2. Open the download once with `CreateFileW(..., GENERIC_READ, FILE_SHARE_READ, ..., OPEN_EXISTING,
     FILE_FLAG_OPEN_REPARSE_POINT | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)`: a named pipe server behind the path can at
     most identify the elevated caller, never impersonate it. The handle must be a regular file on a local disk:
     `GetFileType` is `FILE_TYPE_DISK` (no pipe, no character device), `GetFileInformationByHandleEx` shows no directory and no
     reparse point, `GetFinalPathNameByHandleW` gives a drive-letter path (no `\\server\share`, no `\\?\UNC`) whose volume is
     `DRIVE_FIXED`. Anything else is refused ("The update must be on this PC's own disk").
  3. Copy by that handle into a new file in `updates\` (create-new, no sharing while writing; bounded size: the size
     `latest.json` announced, at most 1 GiB), then close the write handle.
  4. Open the copy again with `GENERIC_READ` and `FILE_SHARE_READ` only, and keep that handle open until `CreateProcessW` has
     returned: while it is open nobody, administrators included, can open the file for writing or deleting. Verify the copy
     **through that handle**, never the original, with `paddock_updater::signature::verify_file` (rule 55: the same code the
     app uses) against the compiled key, including the signed `version:` in the trusted comment; the signed version must be
     newer than the version resource of `$INSTDIR\<main>.exe` (no downgrade).
  5. Start the copy with `/P /UPDATE /R` by `CreateProcessW` (already elevated: no second prompt), with `TEMP` and `TMP` set to
     `$INSTDIR\updates\tmp\` (made like `updates\`), so the installer's `$PLUGINSDIR`, where it extracts and runs its own
     `paddock-setup.exe` and NSIS plug-ins, is in an administrator-only folder too. Then close the handle and exit. S0 confirms
     that `CreateProcessW` succeeds while the read-only handle is open.

  No argument from the caller reaches the installer except through these checks; the relaunch arguments are fixed (none).
- **What this closes:** the window between the app's own check and the elevated run of a file the user can write (review P1-4),
  which matters most when a standard user's malware could hand an administrator a substituted installer.
- **What stays:**
  - A manual install is trusted like any download the person runs.
  - **The split-token `%TEMP%`.** A manually started installer (or an uninstaller) extracts `$PLUGINSDIR`, with
    `paddock-setup.exe` and the NSIS plug-ins it then runs elevated, into the elevated process's `%TEMP%`. For an administrator
    who approves their own prompt, that is their own profile's temporary folder, which the unelevated half of the same account
    can write: a program of that account could swap a file there between extraction and use. Microsoft does not treat the two
    halves of one account as a security boundary, and that program could raise its own prompt anyway. For a standard user who
    types an administrator's credentials, `%TEMP%` is the administrator's, out of the standard user's reach. The update path
    does not have this gap (step 5).
  - The updater of a per-user release (the 1.0 line) still runs the download from the temporary folder itself, which this
    record cannot change, and meets the guard (§5).
  - A build without a key refuses `apply-update` ("Updates are not available in this build").
- The staged copy and `updates\tmp\` are deleted by the next `apply-update` and by the installer's `POSTINSTALL` (`paddock-setup
  clean-staging`); `setup.log` stays.
- `paddock-setup.exe` carries a version resource whose `FileDescription` is "<App> update" (and `ProductName` the app's), set by
  the per-app build (S7), so the UAC prompt and Task Manager name it for the app.

#### 4.3 After the installer

The template's `.onInstSuccess` starts the app again with `RunAsUser` (the desktop user, unelevated) because of `/R`, in the
updating user's session only. When the include refuses (`PADDOCK_ABORT`) during an update started with `/UPDATE /R`, it starts
the still-installed old version the same way (`nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe"`; nothing was
replaced). That start waits for the installer to quit before it shows the Failed state (§4.1 step 4), so it never says "stopped"
while the installer is still running.

### 5. The per-user guard (no migration)

There is no migration: a per-user copy is uninstalled by hand. `paddock-setup guard` is a pure decision
(`paddock_setup::guard::decide` over an `InstallFacts` port, unit-tested with fakes):

- **When:** only on a fresh install, that is when `HKLM\${UNINSTKEY}` does not exist. An update or reinstall of a per-machine copy
  never runs it; a leftover per-user copy found later is handled at the app's start (§6.2).
- **Who blocks:** two accounts only: the account running the installer, and the **desktop user** (the owner of the shell
  process of the installer's session: `GetShellWindow`, its process, its token's user SID; the hive `HKU\<SID>` is loaded
  because that user is signed in). This covers a standard user who typed an administrator's credentials.
- **What blocks:** only `Software\Microsoft\Windows\CurrentVersion\Uninstall\<productName>` (Tauri's `UNINSTKEY`) in that
  account's hive. A file-only hit (`<Local AppData>\<productName>\<main>.exe` with no key), and any hit of another account, is
  logged and does not block (decision O5).
- **The hive scan for the log:** every subkey of `HKU` except the `_Classes` hives and the well-known SIDs (`.DEFAULT`,
  `S-1-5-18`, `S-1-5-19`, `S-1-5-20`); each user's Local AppData is read from that hive's `Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders\Local AppData`
  (expanded with that user's `ProfileImagePath` from `ProfileList`), never from the installer's own `%LOCALAPPDATA%`.
- **The message:** "Uninstall the older <App> first (Settings › Apps), then run this installer again." When the blocking hit is
  the desktop user's and not the installer account's, it adds "It is installed for <account>." The guard never changes, moves
  or deletes anything; `PADDOCK_ABORT` removes only the empty `$INSTDIR` (§2.3).

**An older per-user release meets the new feed** (decision O8: one feed). Its updater quits the app and runs the downloaded
per-machine installer with `/P /UPDATE /R`; UAC appears; the guard stops with the message. When the installer had `/UPDATE`
and `/R` **and the blocking hit is the desktop user's own uninstall key** (not only the installer account's, which with typed
credentials is another account), `PADDOCK_ABORT` starts that per-user copy again as the desktop user (`RunAsUser` on
`<install folder>\<main>.exe`,
the folder taken from the blocking uninstall key's `UninstallString` and checked to hold that file; the start is unelevated,
so a changed value can only start what that user could start anyway). With "install when the app quits" the old updater gives
no `/R`, so nothing is started: the user was quitting. If the file is not there, or the hit is not the desktop user's,
nothing is started and the message ends with "Then start <App> again." The old copy stays installed and usable, with its data. Each app's release notes for its first per-machine version say
all this and how to uninstall without losing data.

### 6. Start at login, always per user

#### 6.1 `UserRunEntry`

`paddock_service::login::UserRunEntry` is an `Autostart` that writes the app's own value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the `Run` backend `paddock-service` already has for user agents, with its
`StartupApproved` handling), with the running executable and the app's start arguments. It:

- never writes HKLM;
- **writes nothing when the token's `TokenElevationType` is `TokenElevationTypeFull`** (the elevated half of a split token,
  which may belong to another account whose credentials were typed, so its HKCU is not the desktop user's): it logs "start at
  login is set from a normal start" and leaves the first-run marker unwritten, so the next unelevated start applies it. A
  `TokenElevationTypeDefault` token (no split: the built-in Administrator, or UAC turned off) is the user's own and writes
  normally;
- replaces `tauri-plugin-autostart` on Windows in every app (the plugin cannot select `CurrentUser`). Linux keeps the plugin;
  macOS keeps `SMAppService.mainApp`.

Each user's first unelevated start applies the app's default (Dyno: on) through it; the marker is per user (the app data
folder), so every user of the per-machine copy gets the default once and their own switch.

#### 6.2 A leftover per-user copy at the app's start

The per-machine app runs `paddock_setup::guard::per_user_copy` for its own account at start (in-process, unelevated, the same
probe as the guard; rule 55):

- If the old copy is **running in this session** (its image in that folder, by `paddock_proc` identity), the app shows a native
  message before the single-instance check, "The older <App> installed only for you is running. Quit it from its tray icon,
  then start <App> again.", and exits. Otherwise the single-instance plugin would hand over to the old copy.
- Otherwise, when the key or the file is there: a Health line, "An older <App> is still installed for this account. Uninstall
  it in Settings › Apps so only one starts at login.", with a button that opens `ms-settings:appsfeatures`.
- The `Run` value has the same name in both copies, so `UserRunEntry` (switch on) rewrites it to the per-machine path at
  start; the old copy no longer starts at login. Nothing else is changed.

#### 6.3 `Run` values on install and uninstall

`PADDOCK_CLEAN_RUN_VALUES` (`paddock-setup clean-run`):

- **HKLM, both registry views, on install and on uninstall:** removes a value under `HKLM\...\Run` whose command names a program
  in `$INSTDIR` (left by an elevated `tauri-plugin-autostart` in `Dynamic` mode, or by hand). `UserRunEntry` never writes one.
- **On uninstall only (not with `$UpdateMode = 1`):** removes the app's value from every loaded user hive (the scan of §5) when
  its command points into `$INSTDIR`. Signed-out accounts keep a value that names a missing program; Windows skips it at sign-in
  and Task Manager lists it as an entry without a program (decision O6).
- **"Delete the application data"** (the template's checkbox) deletes `$APPDATA\<identifier>` and `$LOCALAPPDATA\<identifier>`
  of the account the uninstaller **runs as**. In a per-machine uninstall started by a standard user with an administrator's
  credentials, that is the administrator's profile, so the standard user's data is not deleted even when ticked. Data an app
  keeps under another name (Dyno: `%LOCALAPPDATA%\Dyno`) is never deleted by the template. Other users' data is never touched.
  Each app's user guide says which of its data the box removes.

### 7. Failure modes

| Failure | What happens |
| --- | --- |
| Fresh install, per-user uninstall key for the installer's account or the desktop user | guard message; empty `$INSTDIR` removed; error level 2 (§5) |
| Fresh install, a file-only hit or another account's per-user copy | logged; the install continues; §6.2 handles it at that account's next start |
| Update or reinstall with a per-user copy somewhere | no guard; §6.2 at that account's start |
| A per-user release's updater runs the new installer | UAC, then the guard message; the old copy is started again (with `/R`) and is unchanged |
| Manual upgrade (reinstall page), or "Uninstall" on the same version | the old uninstaller runs in place, acts as an update: shortcuts, `Run` values, services and the helper stay; the HKLM key and the files go and are written again (§2.4) |
| The new installer cancelled or failing after the in-place uninstall | a half-removed app (no files, no HKLM key; shortcuts and `Run` values point at nothing; a service whose folder is gone); running the installer again is a fresh install and repairs it; Dyno's helper removes itself after one hour if the app stays missing (§2.4) |
| `%ProgramData%\Paddock` (or any folder under it) pre-created by a standard user, with junctions or links | not used: nothing elevated stages, logs or reads there (§2.5) |
| `$INSTDIR\updates\` exists with another owner or as a reparse point | renamed aside and made again (§2.5) |
| The download is a pipe, a device, a UNC or network path, a directory or a reparse point | `apply-update` refuses: "The update must be on this PC's own disk"; the old version starts again with that sentence |
| The hand-off wait runs out | the app exits without starting itself; the next start waits for the installer's mutex and then shows the outcome (§4.1) |
| UAC declined (update) | the old version starts again: "Windows permission was declined", Install again |
| UAC declined (manual install) | nothing runs |
| The download changed after the check | `apply-update` refuses on the protected copy; the old version starts again with the reason |
| An older or equal version handed to `apply-update` | refused ("not newer than the installed version") |
| The installer stops after the hand-off (stop, folder) | `PADDOCK_ABORT` starts the old version; once the installer has quit it shows "The installer stopped before it finished" (§4.1 step 4) |
| A standard user without administrator credentials | the update cannot be installed; the status says an administrator must install it |
| A listed program does not end through Restart Manager | the fallback by handle with `SeDebugPrivilege` (§3 step 2) |
| A listed program still runs after both steps | exit 11; message; nothing copied; the log lists image, pid, start time, session, method |
| `SeDebugPrivilege` cannot be enabled | logged; what is still running is reported as above |
| A process with the same file name outside `$INSTDIR` (another copy, the helper's protected copy) | not touched; logged as skipped |
| A pid reused between listing and ending | impossible: Restart Manager's `RM_UNIQUE_PROCESS` and the held handle carry the start time |
| Install folder outside `%ProgramFiles%` (page, `/D=`, or an existing folder with another owner) | refused by `check-dir` with the reason; empty `$INSTDIR` removed |
| `quiet` configured, the key or the stop list differing from the config, `paddock-setup.exe` missing | C91 and the ADR 0039 artifact gate fail the build |
| The app started elevated (`TokenElevationTypeFull`) | no `Run` write, logged; the next normal start applies the switch |
| The old per-user copy running in the same session when the per-machine app starts | the message of §6.2; the app exits |
| Another session's app ended by an update or uninstall | not restarted; that user starts it again or it starts at their next sign-in |
| Uninstall with signed-out users who had start at login | their `Run` value stays and names a missing program (O6) |

### 8. Checks

- **C91 (per-machine installer contract)**, over every app with `installMode: perMachine`: updater mode `passive`; the hooks
  call the macros in the hooks of §2; the synced include and the generated stop list are current, and the list equals
  `mainBinaryName` plus `externalBin` minus the declared exclusions; the app's `PADDOCK_SETUP_UPDATE_KEY` equals
  `plugins.updater.pubkey`; the app declares `InstallScope::PerMachine`; no `tauri-plugin-autostart` on the Windows target.
- **ADR 0039 artifact gate:** the built NSIS installer requests administrator level and contains `paddock-setup.exe`, whose
  `--self-test` names the app's identifier and key fingerprint, and the include's version string.
- **Unit tests:** the guard decision (fresh install only; the two blocking accounts; key versus file; the skipped hives; Local
  AppData from the hive); the reinstall marker decision and `same-folder` (case, trailing backslash, an 8.3 short name, a folder
  that cannot be opened); `stop_all` over a fake Restart Manager and a fake process table (the
  `Dyno Helper` prefix case, the fallback only for what RM left, the privilege enabled only around the end and dropped again);
  `apply-update` over a fake file system (a changed source, a bad signature, an older version, a folder with a wrong owner, a
  reparse point, a pipe or device or UNC source, the copy held read-only until the start returns), and **"`%ProgramData%\Paddock`
  pre-created by a standard user"**: with that folder made by another owner and holding a junction to a system folder, a run of
  `apply-update` and of the log writer touches nothing under `%ProgramData%` and writes only under `$INSTDIR\updates\`, plus Windows tests with real children of `paddock_proc::testchild` in a temporary folder (ER-14: only
  processes the test started); `UserRunEntry` over the fake `System` (HKCU only; nothing for `Full`; writes for `Default`); the
  updater's scope rules and the hand-off record.

### 9. The reusable VM suite

`scripts/vm-smoke` gains a Windows per-machine suite, parameterised by the app's `paddock.ci.json` (product name, stop list,
service, helper hooks), run in the local Windows VM under the ADR 0040 GUI session with two accounts (`paddock-admin`,
`paddock-test` standard) and one signed-out account. Local only, never CI (ER-14). UAC prompts are driven by the GUI automation:
the suite's VM image sets `PromptOnSecureDesktop = 0` so the prompt is on the interactive desktop that the automation can see and
type into; PM-17 runs once with the secure desktop on, by hand. The suite belongs to the `vm-smoke` harness and lands from its
home (ADR 0042).

| # | Case | Passes when |
| --- | --- | --- |
| PM-1 | Fresh install as an administrator | files in `%ProgramFiles%\<App>` with `paddock-setup.exe`; HKLM uninstall key; all-users Start menu shortcut; the started app is not elevated and runs as the desktop user; the first-run default wrote HKCU `Run`; no HKLM `Run` value |
| PM-2 | Fresh install as a standard user with an administrator's credentials | as PM-1, and the app, its `Run` value and its data belong to the standard user; the administrator's hive is unchanged |
| PM-3 | Guard on a fresh install: per-user key for the installer's account; for the desktop user (PM-2's set-up); file only; another signed-in account; a signed-out account | the first two stop with the message and leave no key, shortcut or file except `$INSTDIR\updates\setup.log`; the other three are logged and install |
| PM-4 | The last per-user release updates from the feed: Install, then "install when the app quits" | Install: the app quits, UAC, the guard message, the per-user copy starts again, its data intact; on quit: the same without the restart |
| PM-5 | Uninstall the per-user release by hand ("Delete the application data" unticked), then install per machine | the data folder survives; the per-machine app reads it |
| PM-5b | The same with the box ticked | the suite records which folders went (expected: only `%APPDATA%\<identifier>` and `%LOCALAPPDATA%\<identifier>`) and that the app's own data folder stayed |
| PM-6 | Two users with Fast User Switching, one updates from the app | one UAC prompt (for `paddock-setup.exe`); the other session's programs end through Restart Manager (log: pid, start time, session, method) and are not restarted; the files are replaced; the updating user's app restarts unelevated; switching back, the other user starts the new version from the Start menu with its data; no orphan process |
| PM-7 | A standard user updates | the "administrator must install" text; with credentials it installs and restarts as the standard user; Cancel starts the old version again with "Windows permission was declined" |
| PM-8 | Install on quit | not offered |
| PM-9 | Uninstall with two signed-in users and one signed-out user | every session's programs end; HKLM key and folder gone; both signed-in users' `Run` values and any HKLM `Run` value naming the app gone; the signed-out user's value stays (O6); every user's data stays |
| PM-10 | Reboot with start at login on for both users | at each sign-in: one app, one of each sidecar, the tray icon, nothing elevated, no HKLM `Run` value |
| PM-11 | The app started with Run as administrator (split token), and as the built-in Administrator | the first writes nothing and logs it; the second writes HKCU `Run` |
| PM-12 | Manual upgrade: run the next version's installer by hand, keep the reinstall page's default | the old uninstaller runs in place as an update: shortcuts, all `Run` values, the service or helper stay; the log says "treated as an update"; repeated with "Uninstall" chosen on the same version, and with `$INSTDIR` given as an 8.3 short name |
| PM-12b | PM-12, cancelling the new installer right after the old uninstaller ran in place | a half-removed app as in §7; running the installer again repairs it (shortcuts and `Run` values work again); Dyno: with the installer not run again, the helper removes itself after one hour |
| PM-13 | The download replaced in the user's temporary folder after the check, before Install | `apply-update` refuses; the old version starts with the signature sentence; nothing installed |
| PM-14 | A per-user copy left beside the per-machine one: not running, then running in the same session | the Health line; then the message and exit; nothing changed |
| PM-15 | An HKLM `Run` value naming the app, written before an install and before an uninstall | removed both times; a value naming another program stays |
| PM-16 | The installer stops after the hand-off (a listed program kept alive by a test helper that ignores Restart Manager and is protected from the fallback) | exit 11 message; the old version starts while the installer runs, shows "An update is being installed", and only after the installer has quit shows "The installer stopped before it finished" |
| PM-17 | PM-6 with the secure desktop on, by hand | the same outcome |
| PM-18 | `%ProgramData%\Paddock\<identifier>\updates\` pre-created by the standard user, holding a junction to `C:\Windows\System32`, then a fresh install and an update from the app | both succeed; nothing under `%ProgramData%\Paddock` changed; the log and staged copy are under `$INSTDIR\updates\` |
| PM-19 | The download replaced by a named pipe of the same name, and by a path on a network share | `apply-update` refuses with the disk sentence; the old version starts again |

An app adds its own cases (Dyno: the helper, in its amendment).

### 10. Parallelism and cost (rule 51)

`stop_all` uses one Restart Manager session over a handful of files and waits on handles, never by polling (ADR 0024); the
fallback reads the process table once. The guard reads a handful of registry keys and files. `apply-update` copies one file of
tens of MB once and hashes it once for the signature. None adds more than a few seconds to an install.

## Work packages

Agent-days for one agent. **Shared** marks where the code lives. The `crates/` packages are additive (a new crate, new
modules, a new enum with a `PerUser` default), so no other app must change with them (rule 43); they still follow family ADR
0022 §G.7 (not inside another app's release window).

| WP | Package | Shared | Owned paths | Depends on | Days |
| --- | --- | --- | --- | --- | --- |
| S0 | VM spikes: embedding via `${__FILEDIR__}`; the reinstall marker (`$EXEDIR` in place, `$UpdateMode` read after the hook); whether an elevated administrator opens another account's process with `PROCESS_QUERY_LIMITED_INFORMATION` without `SeDebugPrivilege`; `CreateProcessW` with the read-only handle open; `TEMP`/`TMP` moving the child installer's `$PLUGINSDIR`; the `Global\` mutex from the installer and from `apply-update`; `Quit`, error level and `RMDir` from `PREINSTALL` in passive and `/UPDATE`; `RunAsUser` from an abort; Restart Manager across sessions as an elevated administrator; `SeDebugPrivilege` enable and drop; `ShellExecuteExW` `runas` cancel code; the desktop user from the shell window; `PromptOnSecureDesktop = 0` with the GUI automation; that a per-user uninstall keeps the app's own data | – | `docs/research/` note | – | 2.5–3.5 |
| S1 | `paddock_proc::installed::stop_all`: Restart Manager first, the fallback by handle, the report; unit and Windows tests | crates/ | `crates/paddock-proc/src/installed.rs` (+ one `lib.rs` line) | S0 | 2–3 |
| S2 | New crate `paddock-setup` (new-crate skill): `check-dir`, `guard` (two blocking accounts, the hive scan), `per_user_copy`, `clean-run` (HKCU hives, HKLM both views), `is-reinstall` (fallback), the log, `--self-test` | crates/ | `crates/paddock-setup/` (+ the workspace line) | S1 | 3–4 |
| S3 | `apply-update` and `clean-staging`: `$INSTDIR\updates\` (§2.5), the local regular file opened for identification, the copy, the read-only hold until the start, the signature with the compiled key, the version rule, `TEMP`/`TMP` for the child, the mutex; the version resource | crates/ | `crates/paddock-setup/src/apply.rs`, `crates/paddock-setup/build.rs` | S2 | 2.5–3.5 |
| S4 | The include and its generated stop list: the six macros, `.onVerifyInstDir`, the reinstall marker, the abort clean-up and restart; `render` for `PerMachine`; sync tests | crates/ | `crates/paddock-setup/nsis/`, `crates/paddock-service/src/render.rs` | S2 | 2–3 |
| S5 | `paddock_service::login::UserRunEntry` (`TokenElevationType`) | crates/ | `crates/paddock-service/src/login.rs` | – | 1–1.5 |
| S6 | `paddock-updater`: `InstallScope`, `ElevatedHandoff` installer, the hand-off record and its Failed state at start, install-on-quit refused, the administrator and declined texts; the updater panel | crates/, ui/ | `crates/paddock-updater/src/{state.rs,updater.rs,record.rs,tauri.rs,tauri/native.rs}`, the updater component in `ui/projects/paddock-ui` | S3 | 3–4 |
| S7 | Build and stage `paddock-setup.exe` per app with its key: the `./dev`/workflow step in each per-machine app's Windows bundle job, `check-bundle`, the ADR 0039 gate entries | tools/, .gitea/ (integrator) | `tools/paddock-dev` bundle step, `conformance/release/` | S2 | 1.5–2.5 |
| S8 | Conformance C91 | conformance/ | `conformance/src/checks/C91-per-machine.mjs` | S4, S6, S7 | 1–1.5 |
| S9 | The Windows per-machine suite (PM-1 to PM-19, PM-12b) in `scripts/vm-smoke`, its `paddock.ci.json` fields, the VM image setting, the UAC automation, the testing-manual page | scripts/ (vm-smoke home) | `scripts/vm-smoke/` | S4 | 4.5–5.5 |
| S10 | New apps: `scripts/new-app.mjs` writes the per-machine config, the hooks, the scope and the bundle step | new-app | `scripts/new-app.mjs`, `templates/` | S4, S6, S7 | 0.5–1 |
| D | Dyno's adoption (its amendment PM-a to PM-f: config, hooks, scope, `UserRunEntry` and the start check, helper steps, docs, the rule-54 dispatch, its VM runs) | Dyno | see the amendment | S4–S7 | 7–10 |

**Shared: 23.5 to 33 agent-days; Dyno: 7 to 10; in all 30.5 to 43.** Revision 2 said 29 to 41.5 (revision 3 adds the
`$INSTDIR\updates\` staging, the file checks and the mutex to S3, three spikes to S0 and three VM cases to S9); revision 1 said
14 to 20.5; the difference from revision 1 is the
elevated hand-off (S3, part of S6), the reinstall marker, Restart Manager plus the fallback, the per-app build of
`paddock-setup.exe` (S7), UAC automation and six more VM cases (S9), and the rule-54 dispatch. Order: S0 → S1 → S2 → (S3 → S6)
and S4 → S7 → Dyno PM-a → PM-f (dispatch) → VM runs, with S5, S8, S9 and S10 beside them. With four agents the critical path
S0, S1, S2, S3, S6, Dyno's adoption and two VM passes is about **15 to 20 working days (3 to 4 weeks)**, against the 1 to 1.5
weeks accepted for revision 1's scope; the user chose the full design on 2026-11-04 knowing the review's findings.

### Adoption by the other apps (later, each in its own release line)

| App | Today | Adoption | Days |
| --- | --- | --- | --- |
| Pitcrew | `perMachine`, own hooks | hooks onto the include (the stop list from its config; the service's own `uninstall` stays), the bundle step, `InstallScope::PerMachine` (install-on-quit goes; the hand-off), `UserRunEntry`, PM suite. Its 1.0.x uninstallers lack the reinstall marker, so the first manual upgrade to the adopting version is a full uninstall of 1.0.x: its release notes say to update from inside the app | 2–3 |
| Pitwall | `perMachine`, own hooks | the same as Pitcrew | 2–3 |
| Spotter | `currentUser`, unreleased | per machine from 1.0, after Dyno's VM pass (decision O7); no per-user users exist, so the guard is a safety net | 2–2.5 |
| Debrief | `currentUser`, 1.0.0 to 1.1.1 | in its next feature release (1.2), with the guard, the start check and release notes as for Dyno | 2.5–3 |
| Pacenotes, Sighter | none | per machine from the first release, through the template | in their packaging WPs |
| shell-demo, workbench-demo | demo releases | only if they ship a Windows installer | 0.5–1 |

## Consequences

- Every Windows app installs once per PC, for every user, in a folder only administrators can change; Windows starts each
  user's copy from that user's own `Run` value.
- Every update asks for administrator permission once, for a program in that folder, and runs only an installer whose
  signature that program checked on its own protected copy. Standard users need an administrator. Nothing installs on quit.
- An update or uninstall closes the app for everyone signed in and does not restart it in other sessions; the log names what
  was closed, and the release notes and user guide say so.
- A user with an older per-user copy must uninstall it before the first install; the installer says so and changes nothing.
- One crate, one include and one `Autostart` serve every app, so a fix to how sessions are stopped, how updates are handed to
  the installer or how login entries are written lands once (rule 55). Pitcrew and Pitwall's hand-written hooks go when they
  adopt.
- The UAC prompt names an unsigned `paddock-setup.exe` until the family has a code-signing identity.

## Decisions (the user, 2026-11-04)

| # | Question | Decision |
| --- | --- | --- |
| Scope | The smaller slice or the full design | **Full design now** |
| O1 | Ask programs to quit before ending them, or end them at once | **End other sessions' programs** (Restart Manager's shutdown asks first, then forces; no extra stop event) |
| O2 | Other sessions after an update | **Not restarted**; said in the release notes and the user guide |
| O3 | A standard user's update | **Keep the Install button**, with the elevated hand-off of §4.2 |
| O4 | Install on quit on per-machine Windows | **Off entirely** |
| O5 | Per-user copies of other accounts, signed in or not, and file-only hits | **Log only** |
| O6 | Signed-out users' `Run` values at uninstall | **Leave them** |
| O7 | Spotter per machine from 1.0 | **Yes, after Dyno's VM pass** |
| O8 | One update feed for the last per-user release and the first per-machine one | **One feed**, with PM-4 as corrected (the per-user copy is started again) |

## Amendment 1 (2026-11-05): stopping, the installer lock, and the trust of `apply-update`

Written after S0's measurements and the independent reviews of `paddock-setup` (branches `feat/paddock-setup-2`,
`feat/per-machine-stop-and-include`). It replaces §3's order of steps, §4.1 step 4's mutex and the "no argument is trusted"
detail of §4.2; everything else stands. The user's decision of 2026-11-05 is kept: **one family update key, no per-app keys**.

### §3 (Stopping every session): replaced

Restart Manager cannot end other sessions' programs (S0: `RmShutdown` returns 350 and ends nothing whenever another session
holds a file; confirmed on the two-user VM run). The stop is, in order:

1. the app's graceful stop where it has one (Dyno: `dyno-agent stop`, and `WM_CLOSE` to windows in the caller's own session,
   bounded);
2. then forced end by handle with `SeDebugPrivilege` enabled only around open and terminate, identity and folder checked,
   bounded;
3. Restart Manager only as a check afterwards, filtered to the listed images (it also lists system components that map the
   files, for example `XtaCache` on Arm);
4. each adopting app states in `windows/per-machine.json` (`survivesForcedEnd`) why each program in its stop list survives a
   forced end, and C91 requires it.

### §4.1 step 4 (the installer lock and the app's signal): replaced

The named mutex in `Global\` is dropped. Any authenticated account can create objects in `Global\` (only file mappings and
symbolic links need `SeCreateGlobalPrivilege`), and waiting on a mutex acquires it, so a standard user could squat the name or
hold the lock and block every later update. The new rule is that **nothing an unprivileged account can open can be held by it,
and nothing it can make decides anything**:

- **Exclusion between elevated processes** is a mutex and a hand-over event inside a **private namespace** whose boundary
  descriptor is Administrators (`CreateBoundaryDescriptor`, `AddSIDToBoundaryDescriptor`, `CreatePrivateNamespace`). Only an
  elevated process can create or open the namespace, so the names cannot be squatted, and the objects grant access to SYSTEM
  and Administrators only.
- **Admission** (`apply-update`) looks only at those objects: busy when the lock is held, or when the hand-over event exists and
  is unset. `apply-update` creates the event unset, starts the installer, gives the lock up and waits (no clock) for the
  installer's `hold-installing` to take the lock and set the event, or for the installer process to end; so there is no gap in
  which a second update is admitted. A crashed installer closes its handles and the objects vanish with them. An installer
  that ends before it holds the lock makes `apply-update` exit 31, and the app's Failed path starts the old version again.
- **The app's signal** is a manual-reset event `Global\<identifier>.installing`, made unset by `apply-update` (or by
  `hold-installing` for an install started by hand) and set when the install is over. Authenticated Users get
  `SYNCHRONIZE | READ_CONTROL` only, so the app can wait on it and read its owner and can hold or change nothing. The app and the
  installer judge its owner (SYSTEM, Administrators or TrustedInstaller) and ignore a foreign object. **The signal never
  decides admission**: a standard user's handle can keep a dead install's signal alive, and a squatted signal must not open the
  gap. A squat costs the app its signal and nothing else.
- The app waits with a **required bound** (`wait_for_installer(identifier, bound)`); a timeout means "unknown" and the
  hand-off record settles it at the next start. The 60 s grace rule is dropped.

### §4.2 (the trust of `apply-update`): additions

- **The key is the family's, so the update is bound to the product.** The signed trusted comment's `file:` must start with
  `<productName>_<signed version>_` and end with `-setup.exe` (Tauri signs `timestamp:<unix>\tfile:<name>\tversion:<v>`; the
  NSIS installer is named `<productName>_<version>_<arch>-setup.exe`). The version in the prefix comes from the same signed
  comment that `verify_file` already checked against the announced version, so product `Dyno` does not accept the installer of a
  product `Dyno_Pro`, and another app's validly signed installer is refused (exit 29). The release gate asserts that every
  installer's signed comment carries such a name.
- **Nothing from the caller is opened before it is parsed.** `--file` is a plain local drive-letter path (no UNC, device,
  namespace or stream path, no `..`); the signature is passed as base64 text (at most 1 KiB), not as a path; a link anywhere
  above the file is refused before the open.
- **The staged copy** lives in a per-run folder, is readable by Administrators and SYSTEM only, is refused when it has another
  hard link, and is removed after every refusal.
- **"Not newer"** refuses the same version too. The installed side is the main binary's version resource (only
  `major.minor.patch`), so an announced version with the installed numbers is refused whatever its prerelease part.
- **What stays (addition).** The check of the folders above the file and the open of the file are separate steps: a user who can
  create symbolic links and wins a race between the two could still point the open at a share. That leaks the elevated
  account's network authentication to a server the user names (SMB), and nothing else: the opened handle is still checked to be
  a regular file on a local fixed disk before a byte is read, and the copy is verified on the protected copy. Opening component
  by component would close it and is not done now.
