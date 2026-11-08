# 24. Pitwall's Windows install is the family's per-machine install

- Status: Accepted for 1.1.0 (plan B6b, 2026-11-08). It adopts family ADR 0053; nothing here restates it. (0023 is the chat ADR on the plan branch.)
- Context: Pitwall was `perMachine` since 1.0.0 with hand-written hooks that ran `pitwall-server.exe uninstall` before any check
  of the folder, so a folder the user could write, chosen on the directory page or with `/D=`, put a user-controlled program
  in front of an elevated installer. B6 made the `install` verb refuse such a program; the installer itself had to stop
  running it (the security review of B6, P2-4).

## Decision

Pitwall adopts ADR 0053 as Dyno does, with the shared pieces and no copies (rule 55):

| ADR 0053 part | Pitwall |
| --- | --- |
| Include and `paddock-setup.exe` (S1-S2, S7) | `windows/installer-hooks.nsh` includes `paddock-per-machine.nsh` and the generated stop list; the release workflow stages `paddock-setup.exe` (`windows-pre-bundle`) and reads the built installer (`windows-installer-check`) |
| Stop list from the config | `pitwall-app.exe` and `pitwall-server.exe`, generated from `mainBinaryName` and `externalBin`; no exclusions |
| `check-dir`, the guard, stop-all, the installer lock | the include's macros, in the order the include names; the guard blocks a fresh install over a per-user copy |
| Pitwall's own step | the service: `pitwall-server.exe uninstall` after the folder check and before programs are ended, `install` (with `check-install`'s words) in `POSTINSTALL`; the installer aborts if the removal fails (PW-1); the uninstaller, which cannot abort, first lets the include decide in NSIS alone whether it trusts its folder (the folder HKLM registers, directly under Program Files; otherwise nothing of it is copied or run), then asks `check-dir`, and only then runs the verb; with the tool missing in a trusted folder it removes the service with `sc.exe` and tells the user |
| App service verbs | Settings > Service (Install, Repair, Remove) checks the folder with the same rule (`paddock_setup::wiring::install_dir_ok`) before the unelevated `check-install` and before any prompt |
| Forced end | `windows/per-machine.json`: the app and the server both write through a temporary file and a rename, the cache is SQLite in WAL mode |
| Updater scope (S3, S6) | `InstallScope::PerMachine` with the elevated hand-off (`paddock_setup::wiring` for the contract, the lock and the folder); install on quit is gone |
| `UserRunEntry` | Start at log-in is the per-user `Run` value `Pitwall` on Windows (`start_at_login`); `tauri-plugin-autostart` is not built for Windows |
| C91 | green for Pitwall |

## Consequences

- No Windows build ships whose LocalSystem service runs a program a user can write: B6 refuses it at `install`, this refuses
  it at the installer (plan section 7, the B6 and B6b gate).
- The 1.0.x uninstallers lack the reinstall marker, so the first manual upgrade from 1.0.x is a full uninstall of 1.0.x; the
  release notes already say to install 1.1.0 once by hand.
- The VM suite of the plan (B12) runs the per-machine cases; nothing here has run on a Windows machine.

## VM suite rows (plan B12)

Not guessed here; the per-machine VM round measures each (rule 71 / ER-33: the precondition is checked, and the probe first shows
it can see the thing):

| Row | Question | Precondition to verify first |
| --- | --- | --- |
| PW-1 | Between the service's `uninstall` and `stop-all` in `PREINSTALL`, does the service's recovery (restart on failure) start `pitwall-server.exe` again, so that `stop-all` ends a program the service just started? What does `stop-all` answer (exit 11 or 0) and what does `setup.log` say? | The service is registered with failure actions as `install` writes them, and is running as LocalSystem; a probe that the registration is gone after `uninstall` |
| PW-2 | An upgrade over a running service in two user sessions: after the install, is the service registered once and is the server running in the logged-on user's session only? | Two sessions signed in, the old version's service running |
| PW-3 | Uninstall from a copied `uninstall.exe` in a user-writable folder, next to a planted `paddock-setup.exe` and `pitwall-server.exe`: neither runs, the details say the folder is not the registered one, and the service stays as it was | The folder is writable by the test user; HKLM names the Program Files folder; the planted programs log when run (the harness cases P and Q cover the include; this row covers Pitwall's own hooks) |
| PW-4 | `uninstall` fails with error 1072 (the service is marked for delete while a handle is open, e.g. Services open): the installer aborts with level 43 and its words; running it again after the handle is closed succeeds | Services (`services.msc`) open on the Pitwall service; the probe shows the mark for delete |
| PW-5 | Upgrade from 1.0.x: the 1.0.x uninstaller runs before any 1.1 hook (it may sit in a folder the user could write). What does it run, and from where? | A 1.0.x install; the folder's ACL read first |
| PW-6 | Paddock-setup.exe deleted from a trusted install folder, then Uninstall: the service is removed with `sc.exe` and the user is told | `paddock-setup.exe` absent; HKLM names the folder |
| PW-7 | A corrupt or truncated `pitwall-server.exe` in the install folder (the verb exits non-zero) while the service is registered, then while it is not: the first aborts with level 43 and "uninstall Pitwall first"; the second continues (`sc query` answers 1060) | The server program damaged by hand; `sc query Pitwall` shows the registration state before each run |
| PW-8 | The installer run with `/D=C:\PROGRA~1\Pitwall` (8.3 spelling of Program Files) and with forward slashes: both are refused at install time with level 41 and nothing is installed; the normal path installs and uninstalls | The short name exists on the VM volume (`dir /x C:\`) |
| PW-9 | The HKLM uninstall key deleted by hand, then `uninstall.exe` run from the install folder: it ends at level 44 with "run the Pitwall installer again, then uninstall"; running the installer again restores the key and the uninstall then works | The key is gone (`reg query`), the program files are still there |

## Notes and family follow-ups

- A 1.0.x uninstaller runs before any hook of the new installer, so the release notes tell the user to install 1.1.0 once by hand
  from a protected download and that a 1.0.x install in a folder others could write should be uninstalled first (PW-5). No code.
- ADR 0053's `.onVerifyInstDir` (refuse the directory page's choice as the user types it) is not adopted now; it is a family follow-up
  for the shared include.
- The Run entry's `StartupApproved` state is read by `UserRunEntry::is_on` (off when the user switched it off in Startup apps).
