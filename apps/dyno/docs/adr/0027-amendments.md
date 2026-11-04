# ADR 0027 amendments

The ADR file `0027-helper-for-1-1-0.md` is not on this branch; these amendments apply to it and are merged into it when it lands.

## 2026-11-03: the interactive users' rights on the helper's pipe (security review of WP-1a/1b)

Section 1's endpoint row listed `GRGW` for interactive users (`IU`). `GRGW` is `GENERIC_READ | GENERIC_WRITE`, and `GENERIC_WRITE`
on a pipe includes `FILE_APPEND_DATA`, which is `FILE_CREATE_PIPE_INSTANCE`. Any interactive user could therefore create another
instance of the helper's pipe. The client's owner check does not see that (a pipe's owner is its creator, and the instance a
client reaches is chosen by the OS), so a user could serve forged answers to another user.

The row now reads: SYSTEM and Administrators `GA`; interactive users `0x12019b`
(`FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | FILE_WRITE_EA`, no append, so no create-instance right); network logons
denied. `paddock-ipc` applies the same change to `Access::service_for_interactive_users` (Pitcrew's service pipe). Tests:
`interactive_users_are_never_granted_the_right_to_create_pipe_instances` (any OS) and, on Windows, a second `CreateNamedPipeW`
on a running pipe with only those rights is denied.

## 2026-11-04: SMART is read through the helper first, with an unelevated fallback (amends O2; user)

Decision O2 (section 7) said the Windows SMART path always goes through the helper. Dyno 1.1.0 testing found that nothing ever read
SMART through the helper (`HelperLink::smart` had no caller), the helper's `Info` listed no tools (so Settings showed "Not
installed" after a successful install) and the agent looked for `smartctl` only in places the helper never installs to. The user then
decided, amending O2 (**amended 2026-11-04 (user): helper first, unelevated fallback**):

- With the helper on, every disk is read through it (`HelperFirstWear`, `dyno-agent/src/helper_wear.rs`).
- With the helper off, or when it reads nothing, the agent reads on this computer with any smartmontools it finds: Dyno's own copy,
  then `%ProgramFiles%` (and the x86 folder), the helper's folder, the `InstallLocation` of the installer's registry key (both
  registry views, because the upstream installer writes its uninstall key without choosing one), and the `PATH`
  (`dyno_platform::smartctl_search`). It reads the disks Windows allows an ordinary account (NVMe, per WP-0) and names the others
  as needing Administrator readings.
- The seam: `READ_LOCALLY_WHEN_HELPER_OFF` (`helper_wear.rs`). `false` restores "only through the helper".
- The helper's `Info` lists the tools found in `tools\smartmontools-<pin>\bin\smartctl.exe`; the Settings card says which copy
  Dyno uses (the helper's, Dyno's own, or the person's own install). The install folder is defined once
  (`dyno_platform::tool_install::install_folder`/`smartctl_path`) and used by the installer, the tools list and the helper's reader.
- The installer empties its destination folder first, so a file left by an earlier install cannot pass the post-install check.
- The helper's `Smart` reply carries what the agent's own reader makes, so turning the helper on loses nothing and the history
  continues: `SmartDisk` gained `wear_id` (`DiskWear::id`, model and serial; the one place a serial travels, still no WWN),
  `bytes_written` and `rated_bytes`, filled by the same parser (`parse_smartctl_ssd`, rule 55); `SmartDisk::to_wear()` makes the
  `DiskWear`. `HELPER_API` is 4. Test: `the_helper_path_and_the_agents_own_path_make_the_same_wear_reading_with_the_same_id`.

## 2026-11-04 (helper wiring, WP-0): the fallback policy is one seam for every reading

The user's decision D1 (helper first, then this computer, for every reading) is implemented once, in `dyno-agent/src/helper_first.rs`
(rule 55), and Smart is its first instance:

- `HelperFirst<R: HelperReading>`: helper ready, then the helper's answer; else (or when the helper errors, or its answer holds
  nothing useful) the agent's own reader; else an honest needs-admin reason with the next step. It returns `Sourced { value, via }`.
- `ReadyGate`: `HelperCentre::status()` costs two IPC calls (`Info`, `Audit`), so readiness is kept for 5 s and dropped at once when
  a call finds the helper unreachable or a job changes the helper (`HelperCentre::ready`, `forget_ready`).
- `via: ReadVia` (`dyno-core/src/read_via.rs`) on the reply types of the readings; the UI shows it with `dy-reading-source`.
  App-agent protocol 4 is unreleased, so the fields were appended without a bump.
- `Shared.helper` is over `Box<dyn HelperTransport>`, so tests serve a real helper `Dispatcher` (`tests/common/helper_rig.rs`).
- `SmartLink`, `SharedSmart` and `HelperFirstWear::new` are gone: `helper_first_wear(port, local)` builds the Smart instance.
  `READ_LOCALLY_WHEN_HELPER_OFF` is unchanged. The memory modules (Linux) are the second instance.
- D6 (helper `Traffic` scoped to the caller, administrators see all) is WP-TRF's: the seam only carries the helper's answer.

## 2026-11-04: Windows installs per machine (reverses D1 = L for Windows only; user)

The user decided on 2026-11-04 that Dyno 1.1.0 on Windows installs **per machine**, to `C:\Program Files\Dyno`, for all users,
and accepted that this delays 1.1.0 by about 1 to 1.5 weeks. Start at login is always written per user (HKCU), even when Dyno
runs elevated. There is **no migration**: the user uninstalls the per-user Dyno by hand, and 1.1.0 is the first official
per-machine version ("no backward compatibility before v1"). The mechanism is the family's, in ADR
[0053](../../../../docs/adr/0053-per-machine-windows-installs.md); Dyno is its first adopter. This amendment holds only
Dyno's parts. macOS (DMG) and Linux (`.deb`, AppImage) do not change.

What stays from D1 = L: the helper is still switched on by the user in Settings, with one prompt, and runs from its protected
copy (section 1). What changes is only where Windows puts the app, and with it these points:

- **Packaging.** `tauri.windows.conf.json`: `installMode` `perMachine`; updater `passive` (unchanged); `InstallScope::PerMachine`
  in the composition (no install on quit, one UAC prompt per update, the administrator text for standard users: ADR 0053 §4).
- **Hooks** (`windows/installer-hooks.nsh`, on the shared include):
  - `PREINSTALL`: the per-user guard (ADR 0053 §5: `HKCU\...\Uninstall\Dyno` or `%LOCALAPPDATA%\Dyno\dyno-app.exe`), then
    the all-session stop of `dyno-app.exe`, `dyno-agent.exe`, `dyno.exe`, `dyno-bench.exe` and `dyno-bench-gpu.exe` from
    `$INSTDIR`. It replaces `dyno-agent.exe stop`, which reached only the calling account's agent. `dyno-helper.exe` is not in
    the list: the service runs from `%ProgramFiles%\Dyno Helper\`, and a `dyno-helper.exe` running from `$INSTDIR` is an elevated
    install or removal of a few seconds, which must not be cut short.
  - `POSTINSTALL` (decision H1): when the `DynoHelper` service exists, `"$INSTDIR\dyno-helper.exe" refresh` (a new verb: install
    again, keeping the recorded owner, only if the installed helper's `HELPER_API` differs or it is below `MIN_HELPER_VERSION`;
    otherwise nothing). The installer is already elevated, so this needs no second prompt.
  - `PREUNINSTALL`: the all-session stop; when the service exists, `nsExec` runs `"$INSTDIR\dyno-helper.exe" uninstall`
    directly (the uninstaller is elevated, so the `ExecShellWait "runas"` second prompt goes); then the shared `Run` value clean-up.
    `dyno-agent.exe service uninstall` goes: on Windows the agent has no registration of its own (`login.rs`), and in an
    elevated uninstaller HKCU may be another account's.
- **Start at login.** On Windows `AppAutostart` (`tauri-plugin-autostart`) is replaced by `paddock_service::login::UserRunEntry`
  with the value name `Dyno` (`WINDOWS_APP_VALUE`, which the template's uninstaller also removes) and `--hidden`. The first-run
  default of `feat/dyno-launch-at-login-default` (uncommitted in its worktree on 2026-11-04) lands first and runs through it; its
  marker is in each user's app data folder, so every user of the per-machine copy gets the default once. An elevated start
  writes nothing and leaves the marker unwritten.
- **The helper's app location and the orphan watch.** `install` records `orphan::app_location` of the elevated program, which
  is now `C:\Program Files\Dyno`; no code changes, a test pins the Windows case. With one copy per machine the single recorded
  location is enough, and the watch's file events are on `C:\Program Files`; an update's file swap lasts seconds, far below the
  one-hour grace. No per-user Dyno 1.1 ships, so no per-user hook has to skip the helper: a development per-user build with the
  helper on removes it when it is uninstalled by hand (its `PREUNINSTALL` prompt), or the orphan watch does an hour later; the
  user then turns it on again in the per-machine copy (VM case D-1).
- **R1 (section 3) is closed on Windows.** Turning the helper on elevates `C:\Program Files\Dyno\dyno-helper.exe`, which only
  administrators can change; the per-machine install refuses a folder outside `%ProgramFiles%`. R1 still holds on macOS. The
  protected copy stays (decision H2), so the service, the orphan watch and the update rules of section 4 are unchanged.
- **Data.** User data stays per user in `%LOCALAPPDATA%\Dyno`. In 1.0 that folder was also the install folder; the 1.0
  uninstaller deletes only its own files and removes the folder only when it is empty (Tauri's template: `Delete` per file, then
  a non-recursive `RMDir`), so the history and settings survive the hand uninstall. VM case PM-5 confirms it with the real 1.0
  installer.
- **1.0 meets the 1.1.0 feed.** 1.0's updater downloads 1.1.0 and runs it; UAC appears; the guard stops with "Uninstall the
  older Dyno first (Settings › Apps), then run this installer again."; 1.0 stays installed and usable. The 1.1.0 release notes
  (CHANGELOG and the release page) say, under Windows: "Dyno now installs once for all users of this PC. Uninstall Dyno 1.0
  first (Settings › Apps › Dyno › Uninstall; leave "Delete the application data" unticked to keep your history), then run the
  1.1.0 installer. Updating from inside Dyno 1.0 stops with this message." The user guide (its Windows install section) says the same.

Dyno's VM cases, added to the family suite (ADR 0053 §9):

| # | Case | Passes when |
| --- | --- | --- |
| D-1 | Helper on in a per-user development build, uninstalled by hand, then 1.1.0 per machine | the uninstall's one prompt removes the service, protected copy and data folder; after install, Turn on gives `Ready`; the config records `C:\Program Files\Dyno` |
| D-2 | Helper on across a per-machine update (same `HELPER_API`, then a changed one) | one UAC prompt in both; the first leaves the helper as it was, the second refreshes it in `POSTINSTALL`; `Ready` after; the orphan watch never fires |
| D-3 | Uninstall with the helper on and two users signed in | one prompt; the service, protected copy and data are gone; both users' agents and apps ended |
| D-4 | Reboot with the helper on, two users with start at login | the service starts at boot; each user's tray and agent at sign-in show `Ready` with their own scope (owner in full, the other per U3) |

Work packages (Dyno's part of ADR 0053's row D; the shared S-packages are the family's). They replace the NSIS part of WP-13.

| WP | Package | Owned paths | Depends on | Days |
| --- | --- | --- | --- | --- |
| PM-a | Config, hooks on the include, the stop list, the scope in the composition | `app/src-tauri/tauri.windows.conf.json`, `app/src-tauri/windows/installer-hooks.nsh`, the composition | S3, S4 | 1–1.5 |
| PM-b | `UserRunEntry` on Windows; land `feat/dyno-launch-at-login-default` first; the elevated case | `app/src-tauri/src/login.rs` | S2 | 0.5–1 |
| PM-c | Helper: the `refresh` verb, the Windows `app_location` test for Program Files, the uninstall without `runas` | `crates/dyno-helper/src/{cli.rs,setup.rs,orphan.rs}` | – | 1–1.5 |
| PM-d | Release notes, the user guide's Windows install section, `docs/testing-manual.md`, the Settings text for R1 on Windows | `CHANGELOG.md`, `docs/user-guide.md`, `docs/testing-manual.md` | PM-a | 0.5 |
| PM-e | VM runs: PM-1 to PM-11 and D-1 to D-4, a first pass and a re-run | – | S6, PM-a to PM-c | 1–1.5 |

Decisions for the user (recommended defaults): **H1** refresh an outdated helper in `POSTINSTALL`, which the installer's prompt
already covers (amends D5, which offered it with a second prompt): **yes**. **H2** keep the protected copy in
`%ProgramFiles%\Dyno Helper\` although the app folder is now protected too: **keep** (the service, orphan watch and update
rules stay as tested; running the service from the app folder would make every app update replace a running service).
