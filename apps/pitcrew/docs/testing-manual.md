# Manual tests

Some behaviour depends on the operating system or a real server and cannot be tested in the browser build (Playwright and
the mock backend), with Tauri's mock runtime or against the fake runner: installing the service, the tray, desktop
notifications, runner sessions, a real registration and a real update. Run this checklist on each platform (Windows,
macOS, Linux) before a release, against a release build (`cd app && npx tauri build`) and a Gitea or Forgejo test server
with a repository that has a workflow such as `runs-on: <your label>` with a step that sleeps for a minute.

Write down the platform, build and anything that differs from the expected result. The scripts `scripts/macos-verify.sh`
and `scripts/linux-verify.sh` cover the read-only checks of an installed Pitcrew.

## Service

| # | Steps | Expected |
|---|-------|----------|
| 1 | Fresh machine. Open Pitcrew. | The window opens; a banner says the service is not installed, with the reason and what to do. |
| 2 | Settings ▸ Service ▸ Install service. | The OS's own prompt appears (UAC; Login Items approval on macOS; none on Linux). Afterwards the badge reads "Installed and running" and the banner disappears without restarting the app. |
| 3 | Windows: `sc.exe query Pitcrew`. macOS: `launchctl print gui/$(id -u)/net.decryptology.pitcrew.service`. Linux: `systemctl --user status pitcrew.service`. | The service is running; Windows shows automatic (delayed) start and the recovery actions; Linux shows `Restart=always`. |
| 4 | Kill the service process (Windows: Task Manager as administrator; macOS and Linux: `kill`). | The OS restarts it within about 10 seconds; the window shows "connecting", then reconnects; runners resume and a runner that was running is adopted. |
| 5 | Quit the app from the tray. | The runners keep running and jobs finish. |
| 6 | Reopen the app. | The window shows the current state at once (no polling delay). |
| 7 | Linux headless: `loginctl enable-linger $USER`, log out, push a commit. | The job runs while nobody is logged in. Without linger it does not (documented). |
| 8 | Settings ▸ Service ▸ Uninstall service. | A confirmation explains that runners stop after their jobs; folders and `pitcrew.toml` stay; installing again picks them up. |

## Registering and adopting

| # | Steps | Expected |
|---|-------|----------|
| 9 | Register a runner with a valid token. | The binary is downloaded and verified, the runner registers and the card turns **Idle**; the server lists the runner online. The token appears in no file (`grep` the data folder) and no log. |
| 10 | Register with a wrong token. | A message says what to fix; no runner is left half-registered; the log line is redacted. |
| 11 | Adopt a registered runner folder (stop its old supervisor first). | No registration, the token is not used, the log says the folder was adopted; the runner is supervised. |
| 12 | Start a second supervisor on an adopted folder's binary. | Pitcrew refuses to start it and says which program already runs. |
| 13 | Tick "remember" and register a second runner. | The token is offered from the OS credential store; it is not in `pitcrew.toml` or `state.json`. |
| 14 | macOS and Linux: a job that runs `node --version` or `cargo --version` (tools installed under your home). | The tool is found (login-shell `PATH`). Set `PATH` in `[runners.env]` and check it replaces the login one. |

## Supervision

| # | Steps | Expected |
|---|-------|----------|
| 15 | Kill the runner process. | It restarts after about 10 seconds; the Activity timeline marks a crash. Repeat quickly five times: **CrashLoop**, a notification, restarts paused; Start clears it. |
| 16 | Block the runner's route to the server (firewall rule) for 15 minutes with no job. | After six timed-out polls in 15 minutes: **Stalled**, a notification, a graceful stop and a restart. Unblock: **Idle**. |
| 17 | The same block while a job runs. | The status shows the job and never **Stalled**; the runner is not restarted. |
| 18 | Start the one-minute job, press **Stop** in the first seconds. | The card reads **Draining**; the job finishes; only then the runner stops. |
| 19 | **Drain**, wait for the job, then **Resume**. | **Paused** after the job; no new job is taken; Resume starts it. |
| 20 | Windows: log off while a job runs (interactive runner), log on again. | The runner stops gracefully at log-off (or waits for the job) and starts at log-on; **WaitingForLogon** shows in between. |
| 21 | Windows: switch the runner to the service session. | It restarts between jobs; the runner page warns it runs without a desktop. |

## Updates

| # | Steps | Expected |
|---|-------|----------|
| 22 | Pin an older runner version, then unpin and **Check now**. | The newer version is offered; with *Notify only* a notification appears; with *Automatic* the update starts inside the window. |
| 23 | Apply an update while the one-minute job runs. | The phase reads **Draining**; the job is not interrupted; the swap happens after it; the card returns to **Idle** and the update result reads *Updated*. |
| 24 | Replace the new binary's download with a corrupt file (proxy or edit `.download`). | Verification fails; the installed version is untouched; the result reads *Failed* with the digests. |
| 25 | Make the new runner fail to start (a script in `binary_path`-like test setup that exits at once) or block its server access. | After the exit or 5 minutes, *Rolled back* with the reason; the previous version runs; a notification. |
| 26 | Cancel an update while Draining. | *Cancelled*; nothing changed. |
| 27 | Settings ▸ Updates ▸ Pitcrew ▸ Check for updates (signed build). | Finds a newer release; *Install and restart* replaces the app; the service and runners keep running; the service restarts itself when idle. |
| 28 | Build without the signing key; open the same page. | "Self-update is off", with the reason. |

## Housekeeping

| # | Steps | Expected |
|---|-------|----------|
| 29 | Fill a runner's `work/` and `cache/` folders; open its Disk panel. | A dry-run preview lists what would go and why; nothing is deleted until *Clean now*. |
| 30 | **Clean now** during a job. | It waits for the job; a running job's folder is never touched. |
| 31 | Windows: leave a file open in `work/`. | The cleanup reports it with a reason and retries next time. |
| 32 | Set the minimum free space above the disk's free space. | **Disk low** on the tile, in the tray and as a notification; a cleanup runs under pressure. |

## Tray, window and notifications

| # | Steps | Expected |
|---|-------|----------|
| 33 | Stop a runner; look at the tray (Windows notification area, macOS menu bar, Linux status notifier). | The icon's shape and tooltip change with the worst state (not only colour); the menu lists each runner with actions. |
| 34 | Use the menu's start, stop, drain, resume, update, clean and logs entries. | Each acts as on the runner's page; *Logs* opens the Logs page for that runner. |
| 35 | Close the window; use tray **Open**; macOS: click the Dock icon. | The window is recreated each time and shows current data. |
| 36 | Linux without a tray host. | Closing the window quits the app (the service goes on). |
| 37 | Trigger a stall, a crash loop, an update and low disk; click each notification. | One notification per transition (a runner that stays stalled is announced once); the click opens the matching page. Switch notifications off in Settings: none appear. |
| 38 | Windows: toggle dark and light in the OS; Settings ▸ Appearance. | The window and the tray icon follow. |
| 39 | Right-click, F5, Ctrl+scroll, drag a file onto the window. | No browser menu, reload, zoom or navigation; selecting a path or log line and right-clicking shows Pitcrew's Copy menu. |

## Security

| # | Steps | Expected |
|---|-------|----------|
| 40 | Windows: connect to `\\.\pipe\pitcrew` from a network logon or a service account. | Refused. Another interactive user may connect (documented limitation). |
| 41 | macOS and Linux: `ls -l` the socket and its folder. | Socket 0600, folder 0700, owned by you; another user cannot connect. |
| 42 | Export diagnostics and search the zip for the registration token and the server's runner token. | Neither appears. |
