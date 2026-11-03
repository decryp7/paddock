# The privileged helper: verification on real systems (Dyno ADR 0027)

What was run against the real operating system, as opposed to the fakes in `cargo test`. Local virtual machines only (ER-14, never CI,
never a developer's machine). Each run records the OS build, whether the system's prompt was seen, the states the agent showed, and
the helper's log lines. The harness is `crates/dyno-agent/tests/helper_vm.rs` (`PADDOCK_VM=1`, `--ignored`, `HELPER_ACTION=turnOn|repair|update|turnOff`):
it builds the agent's `HelperCentre` for the machine, prints the state, runs the action as the agent's job (printing each phase) and prints the state again.

## Windows 11 (2026-11-03): done

- **Machine.** `paddock-win11-test` (VMware Fusion, Windows 11 Pro Arm64, `DisplayVersion` 26H2, build 10.0.26300, stable channel: no
  `BranchName`, no `EnablePreviewBuilds`), started from the snapshot `gui-session-tools`, reverted to it afterwards, no snapshot taken.
  Binaries cross-built with `cargo xwin` (x86-64, run under emulation).
- **Who.** `paddock-test` made a member of Administrators for this run (an Admin Approval Mode token, as a real user who is an administrator has),
  signed in to the console (session 1); `PromptOnSecureDesktop=0` so the prompt can be photographed. The agent side ran as a scheduled task in
  that session (`/it`). The prompt was answered by a click through the VM's VNC (the VM's own input device: a synthetic click from inside the
  session is blocked by UIPI).
- **Prompt seen.** Yes, the real UAC prompt: "Do you want to allow this app from an unknown publisher to make changes to your device?" for
  `dyno-helper.exe`, "Publisher: Unknown" (ADR 0027 R1): [turn on](screenshots/helper/windows-uac-turn-on.png), [turn off](screenshots/helper/windows-uac-turn-off.png).

| Step | Result |
| --- | --- |
| Nothing installed | state `Off` ("Administrator readings are not turned on ..."), no endpoint |
| Turn on (UAC, Yes) | phases `check`, `prompt`, `start`, `verify`; ends `Ready` (version 1.0.0 with a minimum lowered to 1.0.0 for this run, see finding 1) |
| Service | `DynoHelper`, `LocalSystem`, delayed automatic start, `sc qprivs`: `SeChangeNotifyPrivilege` only; the running process's token (read with `GetTokenInformation`): exactly `SeChangeNotifyPrivilege` (enabled by default) |
| Files | `C:\Program Files\Dyno Helper\dyno-helper.exe` (owner Administrators; SYSTEM and Administrators full, Users read and execute); `C:\ProgramData\Dyno\Helper\config.json` (schema 2, owner SID of the account that turned it on), `helper.log`, `audit.jsonl` |
| Pipe `\\.\pipe\net.decryptology.dyno.helper` | owner `S-1-5-18`; SDDL `O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x12019b;;;IU)(D;;FA;;;NU)`; the same for an administrator and for `paddock-test` over SSH (a network logon cannot connect: `NU` is denied) |
| Squatted pipe | service stopped, `paddock-test` creates the pipe name first (its own owner). The agent's state is `Missing`: "Something other than Dyno's helper answered on its endpoint, so Dyno did not trust it (... owner S-1-5-21-...-1001, expected S-1-5-18)". The agent sent nothing to it |
| Repair (UAC, Yes) | `check`, `prompt`, `prompt`, `prompt`, `start`, `verify`; `Ready` again |
| Turn off | on Windows through the prompt (see finding 2): the bundled copy removes the service, the installed program and its folder; state `Off` |
| Log lines | `Helper: protected-location check what=the program protected=true`, same for the data folder and configuration, `Helper: serving endpoint=... owner=sid:... version=...`, one `Helper: a connection was accepted account=sid ... admin=true` per call, `a caller could not be identified and was refused` for a client that connects and sends nothing, `install: EnsureFolder, ApplyAcl, CopyProgram (sha256), VerifyProgram, Register, Confirm` with durations; `uninstall: StopExisting, RemoveProgram, RemoveFolder, ConfirmGone` |

### Findings from this run (and what was done)

1. **A development build is older than `MIN_HELPER_VERSION`.** The workspace version is 1.0.0 and the minimum is 1.1.0, so a freshly installed
   helper answers and is `Outdated`; the turn-on job waited its whole minute for a state it could never reach. Fixed: the job now stops at once
   when the helper answers but is `Outdated`, and says so ("The helper started but this Dyno cannot use it. ..."). The `Ready` rows above were
   taken with the minimum lowered to 1.0.0 in a local build; it is 1.1.0 in the commit.
2. **A running Windows program cannot delete itself.** The helper's own `Uninstall` request stopped and unregistered the service but left
   `dyno-helper.exe` in Program Files, and the prompted `uninstall` run from the installed copy failed with `RemoveProgram ... Access is denied`.
   Fixed in the agent: on Windows the elevated `uninstall` runs the bundled copy, which removes the installed one completely, and Turn off on
   Windows goes straight to the prompt. (`uninstall_self_in_place` leaves the program behind on Windows; that is the service crate's to decide.)
3. **One removal failed once** with `RemoveProgram ... left in place while it runs` right after a turn on, and did not repeat in three
   later runs: `StopExisting` returned before the helper process had ended. It deserves a wait on the process's end in `paddock-service`
   (not changed here).
4. **The data folder stays** after removal (`config.json`, `helper.log`, `audit.jsonl`). A deliberate keep of the log, or a leftover: to be decided
   with the owner of `paddock-service`.
5. **One connection per call** writes one `a connection was accepted` line per request; a status poll during a one-minute wait writes
   hundreds. The agent should hold one connection per job (WP-11 follow-up) or the helper should summarise (ADR 0027 section 9 asks for a
   per-minute summary).

Update runs the same code as Repair (install again over the running helper) and was not run separately.

## macOS (Tart `vmgui-base-tahoe-4aaa751f`): not run

The password dialog is `osascript`'s and is answered through the VM's GUI session; the helper must be cross-built for macOS (arm64) and the
VM cloned to a private copy first. Not done in this session. To run: clone, copy `dyno-helper` and the `helper_vm` test binary into the clone,
run `PADDOCK_VM=1 HELPER_ACTION=turnOn helper_vm --ignored --nocapture` from the GUI session, answer the dialog with the VM's VNC, then
`launchctl print system/net.decryptology.dyno.helper`, check the daemon in System Settings > Login Items, the socket's folder
(`/var/run/dyno-helper`, root-owned) and a second standard account; delete the clone.

## Linux (Orb `paddock-gate-arm`, systemd): not run

The machine has no polkit (`pkexec` is missing) and no authentication agent; `pkexec` from the agent's pipe-connected process needs one.
To run: install `polkitd` and `pkexec`, start a text agent on a terminal (`pkttyagent`) or a rule that allows the one command for the test
account, build `dyno-helper` and the `helper_vm` test natively in the machine, run the harness, then `systemctl status dyno-helper`,
`systemd-analyze security dyno-helper`, the socket's mode and its root-owned folder, and a second account.

## Settings card, every state (mock backend, independent review)

`docs/screenshots/helper/card-<state>-<light|dark>.png` for `off`, `silent` (not running), `foreign` (needs repair), `outdated`, `ready` and
`notIncluded`, and `card-turning-on-*` mid-job: generated by `e2e/screenshots-helper.spec.ts` (`SCREENSHOTS=1 HELPER_SHOTS_DIR=...`).
