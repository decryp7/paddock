# scripts/vm-smoke: the scripted VM start check

Rule 54 asks for proof that the release path works before a tag; a green CI run only proves the installer builds. This check
installs the built macOS installer into a **fresh clone of a Tart VM**, cold-starts the app and judges the real process tree,
the app's own discovery files and Quit, then writes a Markdown result file (pass or fail per step, with evidence). It is
the headless part of the Dyno and Spotter VM runbooks (cold start, one instance, no orphan after Quit); with `--gui` it also runs
the rows a person clicks through (Gatekeeper, Login Items, login start, double-click). **Local only: CI never
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
| Cold start | `sudo purge` first (cold disk cache), then `open`. Polls until every process in the config runs, the discovery file names the live helper and the `ready` command answers (guard: `readyTimeoutSec`). Then watches `settleSec` seconds of `ps` snapshots (30 s for Dyno, derived from its 5 s + 8 s spawn timers; each config records why). Every snapshot from the launch on, the readiness polls included, is judged: the peak count of the app and of each helper must not exceed `max`, every helper's parent must be the app (or launchd while Launch at login is on; a helper with `launchdLabels`, Dyno's agent, must be the very pid `launchctl print` reports for the job of the current state, and after Quit with Launch at login off its job must be gone), and the number of distinct processes (pid plus start time) per spec over the whole step must not exceed `max`, so a duplicate that exits before ready, or an agent that dies and is replaced, fails too. It does not check that a window is shown (headless) |
| Second launch while running | `open` again: the same pids, no second app or helper (single instance) |
| Quit with a window open | `osascript quit`; polls until nothing of the app runs; any leftover is listed with its parent (parent 1 = an orphan of launchd); discovery files that must go are gone. A leftover is then ended, after re-checking its pid, start time and command line in the same guest command (rule 53); a leftover that will not go fails the next step |
| Start without a window | `open --args --hidden` (the shared tray start), same judgement, then Quit |
| Relaunch after Quit | start, judge, Quit |
| Launch at login on, off, on | only for an app whose config has a scripted `loginService.on/off`: after each switch no duplicate and only legal parents, and the launchd job must load, else the step is SKIP ("not proven"). **Dyno has none**: the app registers with SMAppService itself, and `dyno-agent service install` is a different (legacy LaunchAgents) route; the headless run does not drive the Settings switch, so this step is SKIP for Dyno here and its login rows run in the GUI mode (`--gui`) |
| Start with the update host black-holed | `/etc/hosts` points the update host at IPv4 and ::1 loopback, where a perl listener accepts, logs and never answers; `purge`, cold start: must still reach ready. The step passes only if the listener was contacted; otherwise it reports SKIP, "NOT EXERCISED" (update checks are off or not on start in this build) |
| Logs | the last lines of each log in the config, for reading |

A step is judged by state (processes, files, exit codes). The only clocks are guards for a stuck step and the observation
window; no check is "sleep, then look".

The headless run installs without the quarantine flag and clicks nothing, so it lists the GUI rows as MANUAL with a pointer to
the GUI mode below; run both before a tag.

## GUI mode (`--gui`)

The same command with `--gui` runs what a person does with the installer, on a clone of the **GUI test image**, still headless:

```sh
scripts/vm-smoke/gui-image.sh                       # once per base digest (about 4 min); --refresh rebuilds, --check re-checks
scripts/vm-smoke/vm-smoke.sh --app dyno --run <id> --gui
```

| Step | How, and what is judged |
|---|---|
| GUI test image ready | the image stamp (evidence), Gatekeeper `assessments enabled`, commands in the `Aqua` session, axdrive AX-trusted |
| Download with Safari | Safari in the guest downloads the installer from the host over http, served on the VM bridge only (the guest's gateway, one file). Safari writes `com.apple.quarantine` itself; it must be Safari's, not yet approved, and the sha256 must match |
| Open the dmg and drag to Applications | `open` the dmg (DiskImageMounter verifies it), then a real pointer drag through the VM's own VNC server (vncdotool, 127.0.0.1): one click on the window first, then the drag; retried at most 4 times, judged by the copy. The copy must still be quarantined. A Finder `duplicate` would leave it translocated and a pointer event posted inside the guest starts no drag (both measured), so neither is used |
| Gatekeeper first launch | Finder opens the app; "Not Opened" > Done > System Settings > Privacy & Security > Open Anyway > "Open?" > Open Anyway > the administrator sheet (the image's public `admin` password); then a window with content; the app must not run translocated, and the quarantine flags must carry the user-approved bit (0x40) |
| Launch at login on | the app's own Settings switch through AX (`gui.launchAtLogin`); launchd has the job, BTM (`sfltool dumpbtm`, inside the guest) says enabled and allowed, and Login Items shows exactly the expected rows (`rows`), no other row of the app (an "Item from unidentified developer" fails) |
| Login Items approval | the item switched off in Login Items (a user who once refused it), then on again in the app: the app must say it needs approval (`needsApprovalText`) and open Login Items; the switch there is turned on and judged by its own value (pressed at most 3 times: the pane rebuilds right after it opens) |
| Reboot as a login item | the app is **quit first** (a running app is restored by loginwindow instead, with a different open event: the next row), then a reboot inside the guest; the image logs in again. The app's log since the reboot must have the hidden-start line and not the window line (`gui.loginStart`), no window may be shown, and the helper must run once with the parent `helperParentAfterLogin`, judged over the settle window |
| Reboot with the app running | the case people meet (restart with a tray app running, "Reopen windows"): judged against `gui.loginStart.restore` (`hidden` or `window`); `null` while the product decision is open, and the step then reports (INFO) what happened |
| Finder double-click | from the app running without a window (after the login start, else the `--hidden` tray start, else not running): a double-click on the app in Applications through VNC must show the window with its content, one app process |
| Launch at login off, Quit | the job goes, the helper is the app's child again; then Quit leaves nothing |

Screenshots are taken **inside** the guest (`screencapture`) and copied next to the result file (`<result>-screens/`). The steps
after Gatekeeper depend on it, so the run stops at the first failing step of download, drag or Gatekeeper.

How faithful it is: the dialogs, Gatekeeper's decision, BTM and launchd are the real ones; the driver is the Accessibility API in
the guest plus the VM's own virtual pointer. The image is not a stock Mac: SIP is off (as in the pinned base) and the guest agent
holds Accessibility, Screen Recording, PostEvent and AppleEvents grants. The app under test gets none of these, but anything that
needs SIP on, or a privacy prompt for the app itself, is not proven here. The user is an administrator; a standard-user run is a
follow-up. Research and measurements: the hand-off note `vm-gui-automation.md` (2026-10-31).

### The GUI test image

`gui-image.sh` clones the pinned base (`run.mjs`: `MACOS_IMAGE`; the image is named after its digest, `guiImageName`), boots it
headless, then: `spctl --global-enable` (the base ships with Gatekeeper **off**); AppleEvents grants for the guest agent to
System Events, Finder, Safari and System Settings in admin's TCC.db (the base already grants Accessibility, Screen Recording and
PostEvent; SIP is off, so the database is writable); answers the screen-capture consent once; installs `axdrive`; writes a stamp
(`/usr/local/share/paddock-vmgui/image.txt`). It then checks the image (Aqua session, Gatekeeper, trust, no prompt for System
Events or screencapture, no consent alert, the grant names the installed agent) and shuts it down. `--refresh` builds the new
image beside the old one and replaces it only after its checks pass; refresh when the base digest changes, when a run reports a
consent alert, or when a run cannot find a control Apple renamed (`Open Anyway`, `background-switch-<name>`). Never refresh while
another agent is running clones of the image. No keys or tokens go into the image.

Host tools (`gui/tools.sh`, cached by hash under `$TART_HOME/.paddock-vm-gui`): `axdrive` from `gui/axdrive.swift` (built with
`swiftc`; it only ever runs in the guest) and vncdotool in a venv from `gui/vnc-requirements.txt`, installed with
`--require-hashes` (MIT; with Twisted and Pillow; a host-side test tool, never shipped).

### Per-app `gui` block

In `apps/<app>/vm-smoke.json`: `windowOwner` (the CGWindowList owner), `contentRegex` (a text of the rendered main window, so a
blank webview fails), `hiddenStart` (the app has the `--hidden` tray start), `guardSec`, and `launchAtLogin`: `label` (launchd),
`open` (AX actions until the switch is there, `["press", app, regex, role]`), `switch`, `save` (or null), `rows`
(`openAtLogin`, `background`: the exact Login Items rows), `needsApprovalText`, `helperParentAfterLogin` (`launchd` or `app`); and
`loginStart` (`log`, `hidden`, `shown`: the log lines that record how the app started; `restore`: how it must start when restored at login, or null). `launchAtLogin: null` skips the login
rows; only Dyno has them today, the others wait for a first `--gui` run with their installer and a decision on their expected rows.
`gui.test.mjs` checks every app's block.

## Still manual

- Windows: SmartScreen's "Windows protected your PC" > More info > Run anyway. See "Not covered here".

## Hard rules it keeps

- **A full checkout.** `./dev` (paddock-dev, the VM slot) is built from `tools/paddock-dev` and the shared crates; a sparse checkout
  without them is refused at the start with the missing paths (`checkoutGaps`), not later as "could not take a VM slot".
- **Nothing on the host's screen**: `--no-graphics` and `CI=1` (Tart then prints the VNC URL instead of opening a viewer); the GUI
  mode's VNC client connects only to the VM's own server on 127.0.0.1 and its URL is never logged; `sfltool` only runs inside
  the guest; the installer server listens on the VM bridge only and closes with the run.

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
are not counted). `stateFiles[]`: a discovery file (`path`, `helper`, `jsonPidField`, `removedAfterQuit`). Also `ready`,
`loginService`, `quit.appleScriptName`, `updateHost`, `readyTimeoutSec`, `settleSec`, `logs`. Adding a helper to an app means
adding it here; `scripts/vm-smoke/judge.test.mjs` checks the shape.

## Not covered here

- **Windows guests** (VMware Fusion, Windows 11 Arm): `scripts/vm-win.sh` on `origin/tools/win-test-vm` drives it (`revert`,
  `start`, `ssh`); the Dyno runbook rows 24a/24d are still partly manual. A Windows port of this check would reuse `judge.mjs`
  and `checks.mjs` with a PowerShell `exec`. Never the personal Windows Fusion VM. **Follow-up for the GUI rows** (not built): the
  SmartScreen prompt appears in the interactive session, which `vm-win.sh ssh` is not, so it needs auto-logon for `paddock-test`,
  an in-session runner (a scheduled task "only when the user is logged on" running dropped job files), UI Automation from
  PowerShell to press More info > Run anyway, screenshots from that session, and the download done by Edge in the guest (Mark of
  the Web). Open questions: SmartScreen's reputation call needs Microsoft's service; Smart App Control on a fresh Windows 11 may
  block an unsigned app outright. Estimate 2 to 4 days.
- **Linux guests**: `tools/macos-vm-smoke.sh --guest linux` (systemd user session, light X session).
- No `HKLM` VM marker, `PADDOCK_LIVE_BROWSER` or conformance check C55 exists on main at the time of writing; the macOS guest uses
  `PADDOCK_DISPOSABLE_VM=1`.

## Tests

`node --test scripts/vm-smoke/*.test.mjs` (CI's `tools` job): the parsers and judges on captured-shape text, and the whole
sequence against a fake guest with seeded faults (a second agent ten seconds late, an agent left after Quit, an app that never
starts). The GUI sequence runs against a fake GUI guest (`gui.test.mjs`) with seeded faults: a drag that does not take, a lost
Login Items press, a translocated first launch, an unidentified legacy row, a window at the login start, a second agent after the
login, Gatekeeper off. They need no VM.

## Update test (`--update-test`, Spotter)

`scripts/vm-smoke/spotter-update-assets.sh --src <checkout> --out <dir>` builds the OLD (`1.0.0-test.1`) and NEW (`1.0.0-test.2`) macOS
builds with TEST keys made for the run (never the family key), a throw-away CA and a local feed folder (`ok`, `badsig`, `wrongkey`).
`scripts/vm-smoke/vm-smoke.sh --app spotter --update-test <dir>` then installs the old build like a user (Safari, drag, Gatekeeper),
trusts the CA in the VM only, serves the feed over HTTPS on the VM bridge, and drives the Updates window: feed down (message, app keeps
running), one signature character changed (refused), a different key (refused), and the success (found, downloaded, verified,
installed, relaunched on the new version, `codesign` passes, one process). A `-test.N` build may take its feed from
`SPOTTER_TEST_UPDATE_FEED` (`update_feed` in Spotter's `lib.rs`); a release build ignores it.
