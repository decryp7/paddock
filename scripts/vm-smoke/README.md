# scripts/vm-smoke: the scripted VM start check

Rule 54 asks for proof that the release path works before a tag; a green CI run only proves the installer builds. This check
installs the built macOS installer into a **fresh clone of a Tart VM**, cold-starts the app and judges the real process tree,
the app's own discovery files and Quit, then writes a Markdown result file (pass or fail per step, with evidence). It is
the headless part of the Dyno and Spotter VM runbooks (cold start, one instance, no orphan after Quit). **Local only: CI never
starts a VM.**

## Run it

One command per app. The installer is a CI run's macOS artifact (`<app>-macOS`, the run must have built bundles; the check
downloads it with `$GITEA_TOKEN`, which is never printed) or a `.dmg` you give it:

```sh
source ~/.zshenv     # GITEA_TOKEN, only for --run
scripts/vm-smoke/vm-smoke.sh --app dyno     --run <dyno-release run id>
scripts/vm-smoke/vm-smoke.sh --app spotter  --run <spotter-release run id>
scripts/vm-smoke/vm-smoke.sh --app pitcrew  --run <pitcrew-release run id>
scripts/vm-smoke/vm-smoke.sh --app pitwall  --run <pitwall-release run id>
scripts/vm-smoke/vm-smoke.sh --app debrief  --run <debrief.yml run id>
# or a file you have:  --installer /path/to/Dyno_1.0.0_aarch64.dmg
```

Options: `--out <dir>` (result files; default `$PADDOCK_VM_SMOKE_OUT`, else `./vm-smoke-results/`, which is git-ignored), `--base
<tart vm>` (clone this local VM instead of the pinned `macos-tahoe-base` image; it must have the guest agent, `admin`/`admin`),
`--artifact <name>` (default `<app>-macOS`), `--keep-vm` (do not delete the clone; for debugging only). Environment: `TART_HOME`
(default `$HOME/.tart`, as in `tools/macos-vm-smoke.sh`; the images here live on d7, so `export TART_HOME=/Volumes/d7sst7/dev-targets/tart-home`), `PADDOCK_VM_MAX_LOAD` (16), `PADDOCK_VM_MIN_FREE_GB` (60), `PADDOCK_VM_MAX` (VMs at once, 1 or 2), `PADDOCK_VM_SLOT_WAIT`, `PADDOCK_VM_LOAD_WAIT`. Exit code 0: no step failed;
1: a step failed; 2: the check could not run (no Tart, load too high, download failed).

To find the run: the last `<app>-release` run of the commit you will tag that **built bundles** (its job list has three
`bundle (...)` jobs and the verdict job says `installers: built`). `GET /api/v1/repos/decryp7/paddock/actions/runs/<id>/jobs`
lists them. A run without bundles has no artifact and the check says so.

## What it checks

| Step | Evidence |
|---|---|
| Install | `sudo spctl --global-enable` first (the pinned image ships with Gatekeeper disabled; the result records `spctl --status`), then `hdiutil` attach, copy to `/Applications`, version from `Info.plist`, `codesign --verify --deep --strict` |
| Cold start | `sudo purge` first (cold disk cache), then `open`. Polls until every process in the config runs, the discovery file names the live helper and the `ready` command answers (guard: `readyTimeoutSec`). Then watches `settleSec` seconds of `ps` snapshots (30 s for Dyno, derived from its 5 s + 8 s spawn timers; each config records why). Every snapshot from the launch on, the readiness polls included, is judged: the peak count of the app and of each helper must not exceed `max`, every helper's parent must be the app (or launchd while Launch at login is on), and the number of distinct processes (pid plus start time) per spec over the whole step must not exceed `max`, so a duplicate that exits before ready, or an agent that dies and is replaced, fails too. It does not check that a window is shown (headless) |
| Second launch while running | `open` again: the same pids, no second app or helper (single instance) |
| Quit with a window open | `osascript quit`; polls until nothing of the app runs; any leftover is listed with its parent (parent 1 = an orphan of launchd); discovery files that must go are gone. A leftover is then ended, after re-checking its pid, start time and command line in the same guest command (rule 53); a leftover that will not go fails the next step |
| Start without a window | `open --args --hidden` (the shared tray start), same judgement, then Quit |
| Relaunch after Quit | start, judge, Quit |
| Benchmarks (Dyno) | `apps/dyno/scripts/bench-smoke.sh <Contents/MacOS>` runs both runners with tiny sizes (ADR 0030 section 5): every kind ends Done or Unavailable with a reason, the numbers are finite and positive, no scratch is left; a VM without a GPU reports Unavailable or a software renderer. It asserts no speed and is not part of the automatic run yet (the Benchmarks page needs a UI driver) |
| Launch at login on, off, on | only for an app whose config has a scripted `loginService.on/off`: after each switch no duplicate and only legal parents, and the launchd job must load, else the step is SKIP ("not proven"). **Dyno has none**: the app registers with SMAppService itself, and `dyno-agent service install` is a different (legacy LaunchAgents) route; driving the Settings toggle needs a UI driver this check does not have (the guest does run `tart exec` in an Aqua session). So for every app this is a manual row today |
| Start with the update host black-holed | `/etc/hosts` points the update host at IPv4 and ::1 loopback, where a perl listener accepts, logs and never answers; `purge`, cold start: must still reach ready. The step passes only if the listener was contacted; otherwise it reports SKIP, "NOT EXERCISED" (update checks are off or not on start in this build) |
| Logs | the last lines of each log in the config, for reading |

A step is judged by state (processes, files, exit codes). The only clocks are guards for a stuck step and the observation
window; no check is "sleep, then look".

## Manual rows (printed in the result as MANUAL, never automated)

- Gatekeeper first launch (right-click > Open) of a quarantined download. Gatekeeper is on in the guest, but the check installs without the quarantine flag, so no prompt appears.
- Login Items approval in System Settings after Launch at login is switched on.
- Anything that needs a click in a window. The VM runs `--no-graphics`; the check never drives the host's desktop, never calls
  `sfltool`, and never opens a window on the host. Use the user's own test VM window for these rows (runbooks in the handoff folder).

## Hard rules it keeps

- A fresh `tart clone` per run, deleted afterwards (also on failure, Ctrl-C and SIGTERM, which also ends in-flight `tart` children). The same VM slot as `tools/macos-vm-smoke.sh` (`./dev ci slot acquire`, an OS lock the kernel frees with its holder), at least 60 GB free under `TART_HOME`, 1-minute load at most 16 (it waits), `nice -n 10`.
- Nothing of the host is touched: no host app, not the personal Windows Fusion VM. The only host files written are the result
  file and a scratch folder (removed). The VM gets the installer through a read-only share. No signing keys: the installer is
  the CI build, the VM never sees `PADDOCK_TAURI_*`.
- Process stops go by identity (rule 53): a leftover is ended only if its command line still matches in the same shell command.
- The guest environment sets `PADDOCK_DISPOSABLE_VM=1`, the marker the other VM smokes require.

## Per-app config

`apps/<app>/vm-smoke.json` lists what the app may start. `processes[]`: `name` (the file in `Contents/MacOS`), `role` (`app` or
`helper`), `max` (default 1), `min` (default 1; 0 for helpers started on demand), `parents` (`app`, and `launchd` for a helper
that a login item may start), `argsPattern` (a regex on the arguments, so the check's own CLI calls such as `dyno-agent status`
are not counted). `stateFiles[]`: a discovery file (`path`, `helper`, `jsonPidField`, `removedAfterQuit`; not required while the login service is on). A process may name a `loginJob`: the launchd job of the login item, whose agent shows in `ps` as a bare `dyno-agent run`; it is identified by `launchctl print` (the pid) and is expected to survive Quit while the login service is on (`loginService.defaultOn`, true since Dyno 1.1.0). Also `ready`,
`loginService`, `quit.appleScriptName`, `updateHost`, `readyTimeoutSec`, `settleSec`, `logs`. Adding a helper to an app means
adding it here; `scripts/vm-smoke/judge.test.mjs` checks the shape.

## Power checks (`--suite power`, Dyno 1.1.0, macOS guest)

`scripts/vm-smoke/vm-smoke.sh --app dyno --suite power --run <id>` installs the same installer, cold-starts Dyno and runs the release
checks of [ADR 0032](../../apps/dyno/docs/adr/0032-power-keep-awake-wake-on-lan-idle-actions-energy.md) (`apps/dyno/docs/releasing.md` step
3b) in the real window. The window is driven through the Accessibility API with `axdrive` (`guest/axdrive.swift`, compiled on the host once,
installed into the clone from the read-only share; the web content of Dyno's window is in the AX tree), the system's password sheet the same
way, and the dashboard drag as real pointer events through the HID event tap. The verdict is always the OS's own state (`power-probes.mjs`)
or the app's own text, never a screenshot.

| Step | Driven by | Judged by |
|---|---|---|
| Admin helper on | Dyno menu > Settings > Dyno > Turn on administrator readings; the SecurityAgent password sheet is filled (`PADDOCK_VM_PASSWORD`, default the image's `admin`; never printed) | `launchctl print system/net.decryptology.dyno.helper`, `/Library/PrivilegedHelperTools/...`, the card reads "On" |
| Keep awake | Power > Keep awake for 1 hour, Stop keeping awake | `pmset -g assertions`: a Dyno hold appears, then is gone (Windows `powercfg /requests`, Linux `systemd-inhibit --list` adapters exist) |
| Scheduled wake | Power > Schedule > Turn on scheduled wake, Add schedule (5 min ahead, Once); cancelled with `dyno wake-schedule cancel` (the row's Change form has no delete button) | `pmset -g sched`: owner `net.decryptology.dyno.*`, 3 to 7 min ahead; gone after cancel |
| Wake-on-LAN | Wake devices > Add by hand (`vm-peer`, `02:aa:bb:cc:dd:ee`), Wake vm-peer | `tcpdump -xx` in the guest: UDP port 9 or 7, payload 6 x FF then the address 16 times |
| GUI tour | menu-bar items (windows `net.decryptology.dyno.menubar.*`), dashboard Edit layout, drag, Done, Quit, relaunch (the order is kept), a CPU + Disk (256 MB) benchmark, Connections map, IP lists, Settings and About | the shared raw-text rules of `paddock-e2e`'s screenshot check (`ui/projects/paddock-e2e/src/screenshot-check.ts`, loaded with node's type stripping; needs the repo's `node_modules`) over the texts the window really shows. Clipped-text and status-chip rules need a DOM and are not claimed |
| Admin helper off | Turn off administrator readings | launchd job and file gone, card "Off" |
| Idle action | a one-minute rule seeded in `settings.json`; the Power page's "Cancel sleep" and "Snooze 30 minutes"; a long `say` file played for the audio wait; a HID event re-arms the rule | `dyno idle`: "Counting down", "Snoozed", "Waiting because audio is playing" (coreaudiod's assertion); the VM is never slept, the OS call is recorded (`/usr/bin/pmset sleepnow`) with the agent's log lines |

The start check also accepts launchd as the agent's parent when launchd itself reports the job `net.decryptology.dyno.agent.ondemand` with that
pid (`parentJob` in `apps/dyno/vm-smoke.json`, Dyno ADR 0026), and requires the job to be gone after Quit.

The restart check (`--suite helper --reboot-check`) records, pass or fail, what names a missing menu-bar item's cause: the shown test per
autosave name (an on-screen Control Center window with that title and a size; on macOS 26 ControlCenter hosts the status items), the app's
`start signal:` line and every `tray check` line, ControlCenter's pid and start time and its log about Dyno (with any "blocked list" or
visibility-0 line). `VM_SMOKE_LOGIN_SWITCH=leave` keeps "Start Dyno at login" as the app set it at its first start instead of switching it on in
the session (default `press`), to compare the two. On Windows, Dyno's tray icons are explorer-owned buttons named after the metrics the app log
says it created (`win-tray-icons.mjs`); a button of Dyno's own window is never one, and step 5 checks that with Dyno's window open.

The macOS "Allow dyno-agent to find devices on local networks?" prompt (UserNotificationCenter) appears after the first Wake-on-LAN; the suite
answers Allow and screenshots it. Parsers and steps are tested against fake guests (`node --test scripts/vm-smoke/*.test.mjs`).

## Windows guest (`run-win.mjs`, Dyno 1.1.0)

`node scripts/vm-smoke/run-win.mjs --app dyno --ui (--run <id> | --installer <setup.exe>)` reverts the Windows 11 Arm test VM
(`paddock-win11-test`, snapshot `gui-session-tools`; the driver is `vm-win.sh`) , starts it, stages the x64 NSIS installer and runs, in the
interactive session of `paddock-test` (a scheduled task, as `docs/testing/windows-gui-session.md` describes), `win-suite.ps1`, then the UI
phase, writes the result in the same format, and stops and reverts the VM (also after a failure; `--keep` leaves it up). Standard-user rights
throughout; `powercfg` needs an administrator, so the suite asks the host through request files and the host answers with the `paddock-admin`
key (only `powercfg /requests`, `/waketimers` and `/a` are ever run).

| Step | Judged by |
|---|---|
| Install (per-machine NSIS, silent, a real UAC prompt answered over the VM's VNC) | precondition: no per-user Dyno in the snapshot; files under `C:\Program Files\Dyno`, the HKLM uninstall entry, no write right for standard users, product version. Evidence kept even on failure: `setup.log`, the NSIS exit reason, recent Application errors, the UAC screenshot (in `evidence/`). A failed install stops the suite: one class (product or check), later steps are "not measured" |
| Cold start, second launch | `Win32_Process`: one `dyno-app`, one `dyno-agent run` whose parent is the app, `agent.json` names it, no new process on a second launch |
| Keep awake | `dyno awake on/off`; `powercfg /requests` lists then drops the Dyno request |
| Scheduled wake | `dyno wake-schedule add/list/cancel` (the wake setting is seeded; `settings.json` is written without a BOM, serde_json rejects one); `powercfg /waketimers` only where `powercfg /a` shows a sleep state, else SKIP with the evidence (this VM has none) |
| Idle action | a one-minute rule in `settings.json`; `dyno idle` shows the countdown, Cancel, Snooze; input is `mouse_event` (a cursor set is not input). No sleep is made; the audio wait is not run (no sound device in this VM) |
| Aero snap (`win-snap.mjs`) | a form dialog (Report a problem) and a viewer dialog (About) are opened through the app's File/Help menu over the WebView2 DevTools port (`--remote-debugging-port`, an SSH tunnel, Playwright `connectOverCDP`), Win+Left is sent by the in-session actor (`win-act.ps1`), and `data-size-mode` on `<html>` must read `manual` |
| Screenshot check on the real DOM (`win-ui.mjs`) | paddock-e2e's `screenshotProblems` over every Dyno window: clipped text, raw identifiers, mock text, colour-only chips |
| Quit | File > Exit: no `dyno-app` or `dyno-agent` left |

**Not built yet (manual):** the tray
right-click menu with the Keep awake and Wake device submenus, dialogs at 150 % (the live display-scale change fails on the VMware display),
Wake-on-LAN on the wire, the "Audio is playing" wait (needs a virtual sound card in the `.vmx`), and everything on Linux (an amd64 `.deb` in a
Rosetta GNOME VM with the polkit prompt).

## Not covered here

- **Windows guests** (VMware Fusion, Windows 11 Arm): `scripts/vm-win.sh` on `origin/tools/win-test-vm` drives it (`revert`,
  `start`, `ssh`); the Dyno runbook rows 24a/24d are still partly manual. A Windows port of this check would reuse `judge.mjs`
  and `checks.mjs` with a PowerShell `exec`. Never the personal Windows Fusion VM.
- **Linux guests**: `tools/macos-vm-smoke.sh --guest linux` (systemd user session, light X session).
- No `HKLM` VM marker, `PADDOCK_LIVE_BROWSER` or conformance check for them exists on main at the time of writing; the macOS guest uses
  `PADDOCK_DISPOSABLE_VM=1`.

## Tests

`node --test scripts/vm-smoke/*.test.mjs` (CI's `tools` job): the parsers and judges on captured-shape text, and the whole
sequence against a fake guest with seeded faults (a second agent ten seconds late, an agent left after Quit, an app that never
starts). They need no VM.

## The verdict, the manifest and the release set

A run is **PASS only when every step its manifest requires ran and passed** (`manifest.mjs`, per OS and suite; `verdict.mjs`). The three outcomes
have their own exit codes, on all three runners:

| Verdict | Exit | Meaning |
|---|---|---|
| PASS | 0 | every required step passed |
| FAIL | 1 | a step failed in the product |
| (error) | 2 | the tool could not run (no Tart, download failed) |
| INCOMPLETE | 3 | nothing failed, but a required step is missing, was skipped or not measured, or its check broke; the result lists what is missing |

A step reports its **class**: `fail` is the product; `check-broken` is the measurement or its control (a dead SSH, a UAC answer that did not take, a
per-user leftover in the snapshot, a missing base image) and never counts as a product FAIL. A literal `ok: true` row is `info` (it cannot make a
pass). Without `--ui` or `--reboot-check` (Windows), `--reboot-check` (Linux) or the right `--suite` the missing steps make the run INCOMPLETE;
`--partial` requires only what the flags ask for and the result says it is not a release verdict. Linux also takes `--only <row numbers>` (the numbers of the report table, for example `--only 7,10,19,23`): the setup (install, cold start, helper on) runs, the chosen rows are driven and required, the preconditions of the chosen rows are checked first and a broken one stops the run as `check broken: <precondition>` (`only-rows.mjs`); the run is partial. Windows has two runs (`--ui`, `--reboot-check`)
because the restart run stops after the install and the start.

The result header states the **installer's sha256**, its source (CI run id or local path) and the **build binding**: `--run` reads the run's head
commit (`GET /api/v1/repos/decryp7/paddock/actions/runs/<id>`, or give `--expect-commit <12 hex>`), and the installed binary must contain the 12-digit
commit the `build-env` action embeds (`option_env!("PADDOCK_BUILD_COMMIT")`). Dyno's `--version` prints the version only (every build reads 1.1.0); the
About page shows the commit, but no CLI prints it: see "What the checks need from the product" in the hand-off.

**The release set for one CI run:** `scripts/vm-smoke/chain-release.sh <ci run id> [results folder]` runs windows-ui, windows-restart, linux-reboot,
macos-start, macos-power, macos-helper and macos-helper-restart. It takes the host's interim VM lock itself (`scripts/vm-lock.sh`) for the whole chain, so a second chain or a `vm-win.sh` call fails at once with `::error::` naming the holder; the old plock lane is gone (the Linux and macOS steps are covered only by this lock). Results go to a stable folder
`<folder>/run-<id>/` (a `.md` and a `.json` per run, screenshots in `evidence/`); `release-verdict.mjs <folder>` prints the release verdict:
PASS only with all seven runs present, not partial, passed, of the same CI run and one installer per OS.

## Rule 71 and release rows (Dyno 1.1.0)

- **MCP from outside the app** (`mcp-judges.mjs`, `mcp-guest.mjs`; guest only). Seeded where the product reads it, no flag: the versioned `mcp.json` in the app's
  config folder (`enabled: true`) and the owner-only `mcp-keys.json` next to it (mode 600 on Unix, as the product demands; `{"version":1,"keys":[{id, client, sha256, created_ms, revoked}]}`; 1.1.0 keeps the list in a file, not the OS credential store, commit 3d03ede45), (a key is
  `pmk_` + 64 hex; only its SHA-256 is stored). The app reads both at start, so it is stopped, seeded and started. Then `DYNO_MCP_KEY=<key> dyno mcp` gets `initialize`,
  `tools/list` (the 10 tools; actions only when allowed) and `tools/call system_summary` (cpu 0 to 100, memory used below total, uptime above 0); the agent is killed and the
  call must be an error naming it; the app is quit and `dyno mcp` must say "isn't open" with exit 1 and `mcp-listener.json` gone or failing pid verification. The
  `writeKeysCommand` writes the file on every OS; no credential-store command is used any more.
- **Quit and stop measuring**: Linux is driven (File menu over AT-SPI, 60 s of no Dyno process, reopen brings it back); the reboot half is not tried in that step, so
  the row stays "check broken" until it is. macOS and Windows: not driven (see below).
- **Release rows** (`release-rows.mjs`): macOS reads `helper-lifecycle.log` as a standard user (`sudo -u nobody`), Linux lists `/var/log/dyno-helper` after the uninstall.
  Not driven yet, each a NOT MEASURED step with its reason, so the run is INCOMPLETE: reinstall over a leftover `uninstall.exe`, the first uninstall after boot, macOS
  Quit-and-stop then reopen (Background Items, Login Items stays off), a hung or paused page, a second user's session, killing the installer, install again.
- **Windows snapshot**: `make-no-dyno-snapshot.sh` makes `gui-session-tools-no-dyno` from `gui-session-tools` with no Dyno (a per-user Dyno makes the per-machine
  installer refuse, exit 2); `chain-release.sh` uses it (`PADDOCK_WIN_SNAPSHOT` overrides) and stops if it does not exist.

**The one command for the full round of a CI run:** `scripts/vm-smoke/chain-release.sh <ci run id>` (needs `GITEA_TOKEN` for the run's commit, or add
`--expect-commit <12 hex>`; each VM takes the host lock).

## Notes for the 1.1.0 rounds

- **Checkout.** The screenshot rows load the shared rules (`ui/projects/paddock-e2e/src/screenshot-check.ts`), which need the `ui/` folder and the repository's
  `node_modules`. Run the round from a FULL checkout (a sparse worktree must include `ui` and the root `package.json`) after `npm ci` at the repository root;
  otherwise those rows say so (a broken check with that next step, `screenshot-rules.mjs`), not a bare "Cannot find module".
- **Tart** always runs with `--no-audio`. The macOS "Idle action: waits while audio plays" row is its own step and is NOT MEASURED, with the reason: the guest has no
  audio device and the host is never used to play a sound (ER-14).
- **Quit.** `Quit Dyno (keeps measuring)` ends the app and keeps the agent; `Quit and stop measuring` ends both (its own row). The macOS Quit steps expect the agent
  to be kept and end it by identity afterwards; Windows and Linux press the exact menu item and wait for the app to go (Windows: up to 15 s).
- **Hidden start.** After a restart Dyno starts with `--hidden`: macOS (`MacGui.ensureMainWindow`), Windows and Linux reopen the running app to show its window before
  the page and Settings rows.
- **Removed in 1.1.0.** Backup health is gone: the Windows Backup versions reading is reported as `not applicable (removed in 1.1.0)`.
- **Linux log folder.** The `/var/log/dyno-helper` row turns the helper on again and checks the unit and a listed log before `apt remove`; otherwise it is a broken
  check. "Turn off, then remove" is not a row: the release notes promise nothing there.
- **Windows Arm.** The boot time is read by CIM, then WMI, `wmic`, the up-time counter and `systeminfo`. The tray rows fall back to the registry list of registered
  icons as information only: a notification area that UI Automation cannot read stays NOT MEASURED.
- **Row 5a** (`login-jobs-row.mjs`, `login-jobs-drivers.mjs`): Start at login is changed in Settings; the Jobs list must show the job, Cancel must end it as Cancelled,
  a second change must be accepted and the switch is put back. A change that ends before the list shows it is NOT MEASURED (a quiet guest answers at once).
- **Install over a running older build** (`upgrade-swap-row.mjs`, `upgrade-swap-prims.mjs`): the older build runs with the helper on, the new one is installed over it
  (macOS: `--installer-before <older .dmg>` with `--suite power`; Linux: `--old-installer <older .deb>`, `dpkg -i`), the app is opened, and within 60 s the running agent
  and the helper must report the new version and no process that started before the install may be left (by pid, path and start time). Preconditions (the old version
  differs from the new, the helper was on) are checked; no older build given or not installable is NOT MEASURED. Windows is not driven (1.0.0 cannot be updated in place by
  1.1.0; no older RC setup driver yet). The version reads come from `dyno status` / the helper's `--version` and are unverified on a live guest.

## Defender evidence (`defender-evidence.mjs`, `defender-rate.mjs`, Dyno 1.1.1 WP-4)

The Windows install row prints the full 1116/1117/1006 text (never cut), one line `Defender: origin / type / source / process / threat id`
(a field the event lacks prints `(not in event)`; the line says when it is the newest detection in the log rather than one since a run started),
`Get-MpThreatDetection` (at most 50, long lines cut with a visible mark), the engine, product and signature versions with the cloud and behaviour
switches (a partial output is printed as unreadable), and the `MPLog-*.log` lines within 30 s of the newest detection (streamed, UTF-16, at most 300
lines per file, each cut at 2000 characters; `STAMPED=n` says whether the lines carried an ISO timestamp at all).

`node scripts/vm-smoke/defender-rate.mjs --installer <setup.exe> [--installs 10] [--snapshot <name>] [--out <folder>] [--plan]` is the detection-rate run. LOCAL
ONLY; it drives only the VM that `scripts/vm-win.sh` allows. It reverts and starts the snapshot, runs an EICAR test file as a positive control, then for
each install reverts again (installs are independent), notes the guest's clock, installs silently as the administrator over SSH (`/S`: a different path
from the GUI/UAC install of `run-win.mjs`, so the rate is for this path), then polls the events for a settle period (`--settle`, default 120 s; a cloud or
ML verdict can come after the installer returns) and counts only the 1116 events at or after that clock that name the program or the installer (a match on
a `Process Name` of `setup.exe` counts too; on a reverted snapshot nothing else runs one). An install that exited non-zero with no detection is `not
measured`. A broken control stops the run at once (`::error::` with the next step); `--force` runs the installs anyway. A write that Defender blocked counts as a detection when its 1116 is there. If the control's 1116 does not name the file with all five
fields, every "no detection" is `check broken` (ER-33). Every phase prints its time and duration. `--plan` prints the schedule and touches nothing. The
unit tests (`defender-evidence.test.mjs`, `defender-rate.test.mjs`) use a fake guest with one cumulative event log; the guest run is scheduled by the
coordinator.
