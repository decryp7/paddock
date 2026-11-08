# 40. A Windows GUI test session as standard: auto-logon, an in-session runner, and vm-smoke's GUI steps on Windows

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03 (ER-21 redesigns, item 2)
  and decided its open points the same day (Decisions of the user), after the independent review approved it (round 1
  and a delta re-review, 2026-11-03).
- Date: 2026-11-03
- Related: ER-14 / rule 7 (tests never touch the user's machine), ER-21 / rule 40, ER-24 / rule 41 (machine load), ER-27 /
  rule 56 (logs explain every failure, on branch `ci/publish-tools`), rule 17 (every dialog is a real OS window, sized by
  its contract), rule 53 (processes by identity), rule 54 (the release path), rule 55 (copies); ADR 0026 (the shared
  dialog API), ADR 0029 (the shell owns the start), ADR 0037 §8 (a real-app smoke per OS, which names this ADR as its
  Windows GUI sibling), ADR 0039 (release gates inspect the artifacts), ADR 0042 (one owner per shared harness). The
  decisions "VMs are local only" (2026-10-28) and "GUI automation into vm-smoke now" (2026-10-31).
- Evidence (read at `origin/main` `9da2ccf59` and on the named branches, 2026-11-03):
  - `scripts/vm-smoke/` on `tools/vm-smoke-unified` (`e478507fc`): the macOS `--gui` mode (`gui.mjs`, `gui-judge.mjs`,
    `gui/axdrive.swift`, per-app `gui` blocks in `apps/<app>/vm-smoke.json`, checked by `gui.test.mjs`). Its README lists
    Windows GUI rows as "Follow-up (not built)" and SmartScreen as still manual. `update-win.mjs` and `win/case.ps1`,
    `win/uia.ps1` already drive Spotter's Updates window through UI Automation **on the desktop of `paddock-test`**, by
    calling `vm-win.sh job`, a script that is not on that branch.
  - `scripts/vm-win.sh` and `scripts/vm-win/` exist only on the unmerged branches `tools/win-test-vm` (6 commits, 10-29) and
    `tools/win-update-test` (one more: `22fef8b07`, 10-31, "vm-win gui-setup and job: auto-logon and an in-session runner
    for UI Automation"). `gui-setup.ps1` sets `AutoAdminLogon` for `paddock-test` and registers a scheduled task
    `PaddockRunner` (`-AtLogOn`, `LogonType Interactive`, `RunLevel Limited`); `logon-runner.ps1` runs dropped `.ps1` jobs
    one at a time and writes `<job>.out` and `<job>.done`, with a heartbeat file.
  - The VM `/Volumes/d7sst7/vms/paddock-win11-test` has the snapshots `clean-baseline`, `dyno-gui-pending`,
    `dyno-1.0-gui-ready`, `gui-baseline` and `pre-dyno-windows-shell` (its `.vmsd`, read 2026-11-03). So the session
    mechanism was built and snapshotted once, but it is not on `main`, not in `vm-smoke`, and the coordinator's own log
    still called it a follow-up on 11-01.
  - Dyno on Windows (`origin/main`): the app's `main.rs:3` has `windows_subsystem = "windows"` for release builds;
    `dyno-agent` and `dyno` (the CLI) have none, so they are console-subsystem programs. The agent's login item is the
    HKCU `Run` key (`apps/dyno/crates/dyno-agent/src/service/spec.rs:6`, `WINDOWS_RUN_KEY`), naming `dyno-agent.exe`. The
    link spawns the agent with `DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`
    (`apps/dyno/app/src-tauri/src/link/ports.rs:151`); Microsoft documents that `CREATE_NO_WINDOW` "is ignored ... if it
    is used with either CREATE_NEW_CONSOLE or DETACHED_PROCESS"
    (<https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags>, read 2026-11-03).
  - `tauri.conf.json` of Dyno: `mainBinaryName` `dyno-app`, `externalBin` `dyno-agent` and `dyno`, NSIS installer.

## Context

Every Windows release check so far has run over SSH, which Windows puts in session 0-like isolation: a non-interactive
logon with no desktop, no taskbar and no notification area. Windows documents that services and non-interactive sessions
cannot show UI to the user (<https://learn.microsoft.com/en-us/windows/win32/services/interactive-services>, read, not
re-checked today). So the checks proved install, a process tree and uninstall, and nothing a person sees.

| Date | Instance | Source |
|---|---|---|
| 2026-10-31 | The auto-logon session and in-session runner were built and snapshotted on `tools/win-update-test` (`22fef8b07`) for Spotter's update test, but never merged; `vm-smoke-unified`'s README calls the same thing "not built" | branches above |
| 2026-11-01 18:50 | Dyno 1.0.0 tagged. Windows VM check: "install, first start and uninstall PASS; GUI untested (no desktop session; follow-up: auto-logon plus an in-session runner)" | coordinator log `handoff/2026-10-29-overnight.md:472` |
| 2026-11-01 21:11 | Spotter: "Windows install/start/uninstall PASS"; the GUI rows were not run | same log, line 485 |
| 2026-11-03 | Dyno 1.0.0 user report 4: the Windows tray popup appears and closes at once. Fix in progress on `fix/dyno-windows-shell` (`8895b72a0`, "keep the tray popover through the blur that follows the tray click on Windows"), with no way to see it work in a real session | user feedback, branch |
| 2026-11-03 | User report: the Dyno agent opens a visible console window. Likely cause (not verified on the VM): the `Run` key starts the console-subsystem `dyno-agent.exe` directly, and Explorer gives a console program a console | evidence above |
| 2026-11-03 | User report: no UI and no tray icon show. Coordinator's hypothesis (unverified): the shortcuts start the wrong program | user feedback |
| 2026-11-03 | User report 7: some dialogs are sized wrong (rule 17). The size depends on the monitor's DPI, which no Windows check ever saw | user feedback |
| 2026-11-03 | Quit on Windows is unverified from the tray: the headless run ends the app with a kill, not with the tray's Quit | VM results |

**Pattern.** A Windows behaviour that only exists in an interactive session (tray, popup focus, windows and their size,
consoles, shortcuts, login start, Quit from the tray) ships unverified, because the only Windows test session has no
desktop. macOS had the same gap until `--gui` (2026-10-31), which found two real Dyno post-reboot bugs on its first run.

**Invariant.** Before a tag, every interactive-session behaviour an app declares on Windows is driven in a real logged-on
session of the test VM and judged by state, with the same step list and judge as macOS `--gui`. A row that is not
automated is listed as MANUAL with a reason, never left out.

## Decision

### 1. One harness: `vm-win` moves into `vm-smoke`

`scripts/vm-win.sh` and `scripts/vm-win/` (from `tools/win-update-test`, which contains `tools/win-test-vm`) merge into
the `vm-smoke` harness and its one home branch (ADR 0042). `update-win.mjs` stops taking `--vm-win <path>` and uses the
module. The VM name stays hard-coded (`paddock-win11-test`), the existing refusal of any other VM stays
(`update-win.mjs`, ER-14), and the user's personal Fusion VM is never named, cloned or started.

### 2. The GUI session is part of the image, not of a run

`vm-win.sh gui-setup` (exists) becomes the Windows counterpart of `gui-image.sh`: from `clean-baseline` it sets
auto-logon for `paddock-test`, registers `PaddockRunner`, and also:
- turns off the lock screen, the screen saver, display sleep and Windows Spotlight prompts for that user (UI Automation
  and `SendInput` stop working on a locked desktop, where the input desktop is Winlogon's);
- turns off the first-run "Let's finish setting up" and notification-centre tips that would steal focus;
- fixes the display at 1920×1080 and records the scale (100 %);
- writes an image stamp (`C:\paddock-test\image.txt`: base snapshot, date, setup script hash) and checks itself (an
  interactive session exists for `paddock-test` with a session id other than 0, `explorer.exe` runs in it, the taskbar
  window `Shell_TrayWnd` exists, the runner's heartbeat is fresh), then takes the `gui-baseline` snapshot.

`--refresh` builds a new snapshot beside the old one and swaps only after its checks pass (as `gui-image.sh --refresh`).
The auto-logon password is the VM's own throw-away one, stored in Winlogon's registry values as today; that is acceptable
only because the VM is NAT-only and reverted after every run (the existing note in `gui-setup.ps1`). Microsoft documents
the `AutoAdminLogon` values (<https://learn.microsoft.com/en-us/troubleshoot/windows-server/user-profiles-and-logon/turn-on-automatic-logon>,
read, not re-checked today).

Every run: `vmrun revertToSnapshot gui-baseline`, start headless (no window on the host's desktop, ER-14), wait for the
heartbeat (a guard, not a sleep), run the steps, collect, revert. Same VM slot, load and disk rules as the macOS run
(`./dev ci slot acquire`, load ≤ 16, `nice`).

### 3. The runner: jobs in the session, judged on the host

The runner stays a dumb executor (drop a `.ps1`, get `.out` and `.done`). It gains: a job id and start and end lines with
durations in `C:\paddock-test\runner.log` (ER-27), the session id and desktop name in every job's first line (so a job
that ran in the wrong session says so), and screenshots taken **inside** the session (`System.Drawing` `CopyFromScreen`),
copied next to the result file as on macOS.

A shared PowerShell library `scripts/vm-smoke/win/gui.ps1` (extends `uia.ps1`) gives the steps their verbs:
- `Get-AppWindows`, `Find-Element`, `Invoke-Element` (exist);
- `Get-TrayButtons`: the notification-area buttons through UI Automation on `Shell_TrayWnd` and the overflow flyout, each
  with its name (the icon's tooltip) and bounding rectangle. Where an app gives its icon a stable `guidItem` (the Dyno
  1.1.0 ask, one icon per metric), `Shell_NotifyIconGetRect` with a `NOTIFYICONIDENTIFIER` reads the rectangle directly
  (<https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect>, read 2026-11-03;
  Windows 7 and later). The exact UIA class names of the Windows 11 tray and its overflow are to be read on the VM in
  WP-3 (unverified here);
- `Click-At` and `RightClick-At`: real pointer input with `SendInput` in the session, never `Invoke` on the tray button,
  because the focus and blur sequence of a real click is the thing under test (the popup bug);
- `Watch-Window`: a UI Automation `WindowClosed` and focus-changed event subscription over an observation window;
- `Get-ConsoleWindows`: top-level windows of class `ConsoleWindowClass` or of Windows Terminal's host whose process belongs
  to the app's tree (by identity, rule 53);
- `Resolve-Shortcut`: the target, arguments and working folder of a `.lnk` (`WScript.Shell` `CreateShortcut`);
- `Get-LoginItems`: the `HKCU\...\Run` values, the user's Startup folder and the user's scheduled tasks;
- `Get-WindowGeometry`: a window's rectangle, client rectangle and `GetDpiForWindow`
  (<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow>, read, not re-checked today).

The host side (`gui-win.mjs`) sends one job per step and judges the job's JSON output with the **same** judges as macOS:
`judge.mjs` for the process tree (peak counts, parents, distinct processes over the step) and `gui-judge.mjs` for windows
and content. A Windows-only judge is added only for what macOS has no form of (consoles, shortcuts, DPI).

### 4. The steps (`vm-smoke.sh --app <app> --os windows --gui`)

The same shape as the macOS `--gui` table; each step is judged by state, and the only clocks are hang guards and declared
observation windows.

| Step | How, and what is judged |
|---|---|
| GUI session ready | image stamp; interactive session of `paddock-test`, not session 0; `explorer.exe`; heartbeat; not locked |
| Install | the CI installer (`<app>-windows` artifact), run with its UI through UI Automation; per-user install folder; Add/Remove entry and version |
| Shortcuts and launch targets | every Start-menu and desktop shortcut the installer made, every login item the app registers: the target is the app's declared main program (`mainBinaryName`), with the declared arguments; each launch target's PE subsystem is `WINDOWS_GUI` (2) (<https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#windows-subsystem>, read, not re-checked today). Fails on a shortcut to a sidecar and on a console-subsystem login item |
| Start from the Start menu | `Start-Process` on the shortcut inside the session; the main window appears with content (`contentRegex`), one app, the declared helpers with legal parents (`judge.mjs`), and **no console window** of the app's tree during the whole observation window |
| Tray icon present | the app's tray button(s) exist with the declared tooltip(s); for an app with several icons, each declared id |
| Tray click opens the popover and it stays | a real left click on the icon; the popover window appears, takes focus, and receives **no** `WindowClosed` during the observation window that follows the click's input sequence; screenshot. Fails on the Dyno 1.0.0 popup |
| Click outside closes the popover | a click on the desktop; the popover closes (blur still works) |
| Tray menu | a right click; the declared items exist; each item that opens a window opens it (re-targeting an open one, ADR 0037) |
| Dialogs at two scales | every dialog in the app's dialog contract (ADR 0026, the manifest ADR 0037 reads) opened once at 100 % and once at 150 %: the window's **logical** size (its physical rectangle divided by `GetDpiForWindow` / 96) equals the contract's size, within 2 physical pixels per edge for rounding at 150 %, is inside the work area, and its last focusable element lies inside the client rectangle (nothing clipped); screenshot each |
| Second launch | the shortcut again: same pids, one window shown (single instance) |
| Start at login | the app's own Settings switch through UI Automation; the login item points at the declared program and arguments; sign out and back in (auto-logon); then: the app runs hidden, its tray icon is there, no window, no console, the helpers once |
| Quit from the tray | the tray menu's Quit; the whole tree ends by itself (no kill), discovery files removed; leftovers listed with parents, then ended by identity |
| Uninstall | the uninstaller's UI; no process, no login item, no shortcut, no install folder left |
| Still manual | SmartScreen's "Windows protected your PC" on a downloaded installer (decision D1) |

The 150 % pass sets the user's scale and signs out and in once; it runs only the dialog step. The observation windows are
per-app values in the config, each with the reason it was chosen (as macOS `settleSec`).

### 5. Per-app config

`apps/<app>/vm-smoke.json` gets a `windows.gui` block beside the macOS `gui` block: `mainProgram`, `helpers` (reusing
`processes[]`), `contentRegex`, `tray` (tooltips or `guidItem`s, `popoverTitle`, `menu` items and what each opens),
`dialogs` (`"from-contract"` or a list), `loginItem` (kind `run-key` | `startup-folder` | `task`, program, arguments),
`observeSec` with reasons, and `manual` (rows not automated, each with a reason). `windows.gui: null` is allowed only for an
app with no window on Windows, with the reason.

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C70** (new), Windows GUI coverage | every app whose `tauri.conf.json` targets Windows: `vm-smoke.json` has a `windows.gui` block (or `null` with a reason); every tray item and every dialog in the app's contract appears in it or in `manual` with a reason | every app (no app has the block) |
| C70, launch targets | every program named by the Windows bundle's shortcuts and by the app's login-item spec (`WINDOWS_RUN_KEY` and kin) is `mainBinaryName` or a program built with `windows_subsystem = "windows"` | Dyno's `Run` key naming `dyno-agent.exe` (console subsystem), if the VM confirms the cause |
| ADR 0039's Windows artifact leg (extended by this ADR) | the PE subsystem of every `.exe` in the built installer that a shortcut or login item names | a console-subsystem launch target in the shipped installer |
| `gui-win.test.mjs` | the whole step list against a fake Windows guest with seeded faults: the popover closes after the click, a console window of the agent, a shortcut to the sidecar, no tray icon, a dialog 1.5× too small at 150 %, a leftover after Quit, a locked session, a job run in session 0 | the harness itself |
| Rule 54 amendment (c) | a tag of an app gated by D2 needs a `--os windows --gui` result for the tagged head with no FAIL, in the steward's tag checklist (and in `./dev release tag` once ADR 0039's tag gate exists) | every past tag |

The tag check is a local result file, because VMs are local only; it is checked by the tag step, not by CI. The result
file names, in its header, the **head SHA** of the tested installer's CI run, the **image stamp** of the `gui-baseline`
snapshot (§2) and the **harness commit** of `vm-smoke` that ran it; the tag step refuses a file whose SHA is not the
tagged head, and prints the stamp and harness commit in the tag's checklist so a later reader knows what was proven with
what.

## Rollout and order

1. **WP-1:** merge `tools/win-update-test` into the `vm-smoke` home branch (ADR 0042), wire `update-win.mjs` to the
   module, docs and tests. Start the Windows Dyno 1.1.0 work on it at once.
2. **WP-2:** `gui-setup` additions (lock, sleep, prompts, resolution), the image stamp and self-check, `--refresh`;
   rebuild `gui-baseline` from `clean-baseline`. The other snapshots are deleted after the user's OK.
3. **WP-3:** `gui.ps1` verbs, read the Windows 11 tray's UIA tree on the VM and record it, screenshots in session, the
   runner's log lines.
4. **WP-4:** the steps from session-ready to Quit and uninstall, with the shared judges, `gui-win.test.mjs`.
5. **WP-5:** dialogs at 100 % and 150 % from the dialog contract.
6. **WP-6:** C70 and the per-app blocks, Dyno first (it carries four of the triggers), then Spotter, Pitwall, Pitcrew,
   Debrief; the rule 54 amendment; ADR 0039's PE-subsystem leg.
7. **WP-7:** the first full Dyno 1.1.0 run, harness fixes from it, a lesson.

WP-1 to WP-4 come before the Dyno 1.1.0 tag, since four of its must-fix items are Windows GUI bugs. ADR 0037 §8 uses the
same session for its Windows tray step once WP-4 lands.

**Rollback.** The GUI steps are a mode (`--gui`); the headless Windows run stays as it is. The snapshot swap keeps the
previous `gui-baseline` until the new one passes.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 merge `vm-win` into `vm-smoke`, wire, tests | 1 |
| WP-2 image hardening, stamp, self-check, refresh | 0.75 |
| WP-3 `gui.ps1` verbs, tray tree, screenshots, runner log | 2 |
| WP-4 steps and shared judges, fake-guest tests | 2.5 |
| WP-5 dialogs at two scales | 1.5 |
| WP-6 C70, per-app blocks, rule 54 (c), ADR 0039 leg | 1.5 |
| WP-7 first Dyno run, fixes, lesson | 1 |
| **Total** | **10.25** |
| SmartScreen and an Edge download (only if D1 says so; the old README's 2-4 day estimate) | (2-4) |

A full Windows GUI run is expected to take 20-30 minutes (two sign-outs, two scales); it is measured in WP-7 and recorded
in `docs/benchmarks.md` (rule 48) before anyone quotes it.

## Consequences

- Windows tray, focus, console, shortcut, dialog-size and Quit behaviour is seen before a tag, by the same judges as
  macOS, so the two OSes cannot drift into two harnesses.
- The VM is Windows 11 on Arm and runs the x64 installer under emulation. Tray, focus, windows, DPI and login items are
  the same; x64-specific code and performance are not proven here (CI's Windows runner covers x64 tests, not GUI).
- The image is not a stock PC: auto-logon, no lock screen, a runner task. The app under test gets no extra rights; the
  user is a standard user (`RunLevel Limited`), which is closer to a real PC than the macOS image's administrator.
- Every new Windows tray item or dialog needs a config line, or C70 fails.

## Alternatives considered

- **WinAppDriver / Appium.** Microsoft's WinAppDriver has had no release since 1.2.1 (2020) and needs Developer Mode (read
  in earlier research, not re-checked today). It also still needs an interactive session. Rejected; UI Automation from
  PowerShell is in the OS.
- **tauri-driver with msedgedriver in the VM.** It drives the webview's DOM, not the tray, the notification area, the
  OS's focus sequence, consoles or shortcuts, which are the failures here. ADR 0037 §8 uses it in CI for the content.
  Kept there, not a replacement.
- **PsExec `-i` into the console session from SSH.** Sysinternals' licence is not on the allow-list and it needs an
  administrator; the runner task does the same with in-box tools. Rejected.
- **The CI Windows runner's desktop.** It would put windows on a real PC's desktop (ER-14) and the user decided VMs and
  GUI tests stay local. Rejected.
- **RDP into the VM.** An RDP session is a different session from the console the auto-logon gives; disconnecting it locks
  that session and UI Automation stops. Rejected.

## Decisions of the user (2026-11-03)

The user accepted the reviewer's recommendations on 2026-11-03. The delta re-review approved the ADR with them.
- **D1:** SmartScreen stays **MANUAL**; the Edge download and "Run anyway" are not automated.
- **D2:** the Windows GUI result **gates the Dyno 1.1.0 tag now**; it gates another app's tags once C70 shows that app's
  `windows.gui` block (not `null`, every tray item and dialog covered or listed as manual).
- **D3:** the **150 % dialog pass runs in every run**.

## Amendment (2026-11-08, proposed): §1 and §3 built on ADR 0042's `vm-smoke` home and probe kit

- Status: **Proposed** (2026-11-08), together with ADR [0042](0042-one-owner-per-harness-and-small-batches.md)'s
  amendment of the same date, which holds the evidence and the mechanism. Revision 2 (same day) answers the independent
  review's round 1.
- What was built, read 2026-11-08 on `test/dyno-vm-harness-1.1-win` (`330b7e356`): the Windows rows of Dyno 1.1.0 run in
  the session of `paddock-test` through an in-session actor (`win-act.ps1`: `SendInput` clicks, keys, foreground, window
  rectangles, screenshots, tray icons) and the webview's DevTools port (`win-cdp.mjs`), judged by Windows-only judges
  (`win-judges.mjs`), from the snapshot `gui-session-tools`. Not built: §1 (`vm-win.sh` is not in git; the runner defaults
  to a scratch path that does not exist; no VM other than the test VM was refused), the image stamp and self-check of §2, `gui.ps1` and `gui-win.mjs` of §3, the
  shared judges of §3, dialogs at 150 % (`run-win.mjs:234` lists them as manual), C70 and the `windows.gui` blocks of §5.
  In the 111 VM reports of 2026-11-05 to 11-08, Windows full runs had 66 harness-caused check-broken steps of 587 (11.2 %;
  macOS 2 of 700, Linux 10 of 248; ADR 0042's amendment, counted by `round-stats.mjs`).

**What changes.**
- **§1** is done by ADR 0042's WP-V1: `scripts/vm-win.sh` and `scripts/vm-win/` from `tools/win-update-test`, with the
  scratch copy's 59-line difference reconciled, enter the `vm-smoke` home; the six `PADDOCK_VM_WIN` defaults become the
  repo path. §1's sentence "the existing refusal of any other VM stays" was wrong: before WP-V1 nothing refused another
  VM (`vm-win.sh` and `prepare.sh` refused only a folder under `$HOME`; `PADDOCK_WIN_VMX` and `PADDOCK_VM_WIN` were
  unchecked overrides). WP-V1 adds the refusal (ADR 0042 amendment §7.4): only `/Volumes/d7sst7/vms/paddock-win11-test`
  can be driven, checked by the driver itself and by the harness, with a fake mode for the scripts' own tests only.
- **§3**: the shared PowerShell library is ADR 0042 §8's `scripts/vm-smoke/kit/guest.psm1`, not a separate `gui.ps1`. It
  takes the verbs of §3 and of `win-act.ps1` and unified's `uia.ps1`, and the kit's in-guest verbs (`Wait-Until`,
  `Get-ProcIdentity`, `Stop-ByIdentity`, `Read-LinesSince`, `Register-Undo`). The in-session actor stays the executor;
  every wait in it is the kit's.
- **§4's steps** become rows of the app's registry (ADR 0042 §9) with `needs` (`desktop-session` is "GUI session ready")
  and are selected with the one `--only` (ADR 0042 §10). Their judges move to the shared `judge.mjs`/`gui-judge.mjs` as §3
  asked, Windows-only judges only for consoles, shortcuts and DPI.

**Work packages, re-estimated** (agent-days):

| WP | Status | Days |
|---|---|---|
| WP-1 merge `vm-win` | not built; becomes ADR 0042 WP-V1 | (in V1) |
| WP-2 image hardening, stamp, self-check, `--refresh` | not built | 0.75 |
| WP-3 `gui.ps1` verbs, tray tree, screenshots, runner log | partly built in `win-act.ps1`; becomes ADR 0042 WP-V5 | (in V5) |
| WP-4 steps on the shared judges, `gui-win.test.mjs` | rows exist with Windows-only judges; the move onto the kit is in WP-V6; left here: the shared judges and the fake-guest test | 1.5 |
| WP-5 dialogs at 100 % and 150 % | not built (manual) | 1.5 |
| WP-6 C70, `windows.gui` blocks, rule 54 (c), ADR 0039 leg | not built | 1.5 |
| WP-7 first full run with the new steps, lesson | Dyno 1.1.0's rounds and ADR 0042's WP-V6 rounds run the rows that exist; none of them runs §4's new steps (shortcuts and launch targets, the popover staying open, dialogs at 150 %, start at login with sign-out), so one run of those after WP-5, its fixes counted in WP-4 to WP-6 | 0.5 |
| **Left here** | | **5.75** |
