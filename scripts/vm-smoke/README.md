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
(default `/Volumes/d7sst7/dev-targets/tart-home`, where the images are), `PADDOCK_VM_MAX_LOAD` (30). Exit code 0: no step failed;
1: a step failed; 2: the check could not run (no Tart, load too high, download failed).

To find the run: the last `<app>-release` run of the commit you will tag that **built bundles** (its job list has three
`bundle (...)` jobs and the verdict job says `installers: built`). `GET /api/v1/repos/decryp7/paddock/actions/runs/<id>/jobs`
lists them. A run without bundles has no artifact and the check says so.

## What it checks

| Step | Evidence |
|---|---|
| Install | `hdiutil` attach, copy to `/Applications`, version from `Info.plist`, `codesign --verify --deep --strict` |
| Cold start, window shown | `sudo purge` first (cold disk cache), then `open`. Polls until every process in the config runs, the discovery file names the live helper and the `ready` command answers (guard: `readyTimeoutSec`). Then watches `settleSec` seconds of `ps` snapshots: the peak count of the app and of each helper must not exceed `max` (a respawn ten seconds late is seen), every helper's parent must be the app (or launchd while Launch at login is on) |
| Second launch while running | `open` again: the same pids, no second app or helper (single instance) |
| Quit with a window open | `osascript quit`; polls until nothing of the app runs; any leftover is listed with its parent (parent 1 = an orphan of launchd); discovery files that must go are gone. A leftover is then ended, after re-checking its command line in the same guest command (rule 53) |
| Start without a window | `open --args --hidden` (the shared tray start), same judgement, then Quit |
| Relaunch after Quit | start, judge, Quit |
| Launch at login on, off, on | for apps whose config has `loginService.on/off` (Dyno: `dyno-agent service install/uninstall`): after each switch the window sees no duplicate and only legal parents; `launchctl print` state is recorded. Apps without a scripted toggle report SKIP (a manual row) |
| Start with the update host black-holed | `/etc/hosts` points the update host at a local listener that accepts and never answers, `purge`, cold start: must still reach ready. It does not read whether update checks are on in the build |
| Logs | the last lines of each log in the config, for reading |

A step is judged by state (processes, files, exit codes). The only clocks are guards for a stuck step and the observation
window; no check is "sleep, then look".

## Manual rows (printed in the result as MANUAL, never automated)

- Gatekeeper first launch (right-click > Open) of a quarantined download. The check installs without the quarantine flag.
- Login Items approval in System Settings after Launch at login is switched on.
- Anything that needs a click in a window. The VM runs `--no-graphics`; the check never drives the host's desktop, never calls
  `sfltool`, and never opens a window on the host. Use the user's own test VM window for these rows (runbooks in the handoff folder).

## Hard rules it keeps

- A fresh `tart clone` per run, deleted afterwards (also on failure, Ctrl-C and timeout). One VM at a time (the same lock as
  `tools/macos-vm-smoke.sh`), load below 30 and disk checked first, `nice -n 10`.
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
  and `checks.mjs` with a PowerShell `exec`. Never the personal Windows Fusion VM.
- **Linux guests**: `tools/macos-vm-smoke.sh --guest linux` (systemd user session, light X session).
- No `HKLM` VM marker, `PADDOCK_LIVE_BROWSER` or conformance check C55 exists on main at the time of writing; the macOS guest uses
  `PADDOCK_DISPOSABLE_VM=1`.

## Tests

`node --test scripts/vm-smoke/*.test.mjs` (CI's `tools` job): the parsers and judges on captured-shape text, and the whole
sequence against a fake guest with seeded faults (a second agent ten seconds late, an agent left after Quit, an app that never
starts). They need no VM.
