# User guide

Reference for everyday use: installing Pitcrew, registering runners, what the screens show, the configuration file and
troubleshooting. For what Pitcrew can do, see [features.md](features.md); per-OS walk-throughs are in
[macos.md](macos.md) and [linux.md](linux.md); for a first look, see the [README](../README.md#quick-start).

- [Glossary](#glossary)
- [Install](#install)
- [First run](#first-run)
- [Registering or adopting a runner](#registering-or-adopting-a-runner)
- [Everyday use](#everyday-use)
- [Updates](#updates)
- [Configuration file](#configuration-file)
- [Files and logs](#files-and-logs)
- [Command line](#command-line)
- [Troubleshooting](#troubleshooting)

## Glossary

| Term | Means | Not to be confused with |
| --- | --- | --- |
| **Runner** | One Gitea or Forgejo CI runner process Pitcrew manages, with its own folder, labels and policies. | The server, or a job. |
| **Service** | The background program (`pitcrew-service`) that owns the runners. It runs whether or not the window is open. | The app (window and tray). |
| **App** | The window and the tray icon. It holds no runner state. | The service. |
| **Drain** | Finish the current job, take no more, then pause (maintenance). Resume starts the runner again. | Stop, which also waits for the job but does not pause. |
| **Stalled** | Polls for tasks keep timing out while no job runs; Pitcrew restarts the runner. | Idle: an idle runner is quiet and healthy. |
| **Crash loop** | The runner exited five times in ten minutes; restarts pause for 30 minutes or until you press Start. | A single crash, which is restarted after a short delay. |
| **Adopt** | Use an existing registered runner folder as it is. | Register, which creates a new registration. |
| **Roll-back** | Putting the previous runner binary back when an update does not come up. | Pin, which keeps a chosen version. |
| **Data folder** | Where the service keeps `pitcrew.toml`, `state.json` and logs. | A runner's own folder. |

## Install

No release is published yet; build the installer yourself ([development.md](development.md)) or take the CI artifacts.

- **Windows 10/11.** Run `Pitcrew_<version>_x64-setup.exe`. Then **Settings ▸ Service ▸ Install service**; Windows asks for
  administrator permission (UAC) once.
- **macOS 13+.** Open the `.dmg`, drag Pitcrew to Applications, open it the first time with right-click ▸ Open (the build is
  not notarised). Then **Settings ▸ Service ▸ Install service** and approve the agent under System Settings ▸ General ▸
  Login Items. Details: [macos.md](macos.md).
- **Linux (x86-64 or ARM64, systemd).** Install the `.deb` or run the AppImage, then **Settings ▸ Service ▸ Install
  service**; nothing is elevated. For a headless machine run `loginctl enable-linger $USER` once. GNOME needs the
  AppIndicator extension for the tray. Details: [linux.md](linux.md).

## First run

The window opens on the Dashboard. If the service is not installed or not running, a banner says so with the reason and
the next step; the window reconnects by itself once the service is up. With no runners the Dashboard offers **Register a
runner**.

The screens (sidebar): **Dashboard** (runner cards, load, history), **Runners** (list and a page per runner), **Activity**
(timeline, jobs, uptime, update results), **Logs**, **Updates** (every runner's version, the default schedule, Pitcrew's
own update) and **Settings**.

## Registering or adopting a runner

**Runners ▸ Register runner** opens the wizard:

1. **Server and token.** The server URL (for example `https://git.example.com`) and a registration token from the server
   (Site administration, organisation or repository ▸ Actions ▸ Runners ▸ Create new runner). Tick "remember" to keep the
   token in the OS credential store for more runners; it is never written to Pitcrew's files.
2. **Name.** A Pitcrew name (`a–z`, `0–9` and `-`, at most 32 characters) and the name the server shows. **Labels**
   decide which workflows run on it, for example `ubuntu-latest:host`.
3. **Folder.** A new folder under the data folder (`runners/<name>`), or **an existing runner folder** to adopt: a folder
   with a `.runner` file is used as it is and the token is not sent. Stop whatever supervised that folder before.
4. **Where it runs (Windows).** The logged-on user's desktop (default; desktop jobs work) or the service session, and
   what an interactive runner does while nobody is logged on.

Pitcrew then downloads the latest runner for the platform, verifies it, writes its configuration, registers it and starts
it. The card shows **Idle** once the server accepts it.

## Everyday use

- **Start, stop, restart** from a runner's card, its page or the tray menu; stop and restart wait for the current job.
- **Drain** before maintenance: the card reads *Draining* until the job ends, then *Paused*. **Resume** starts it again.
- **Read the status.** Idle, Busy, Draining, Paused, Stopped, Starting, Degraded (the server is unreachable), Stalled,
  CrashLoop, Updating, NotRegistered, WaitingForLogon. Anything needing you shows a badge on the tray icon.
- **Logs.** Pick a runner, search in plain text, follow the end of the log. The runner's `supervisor:` lines say what
  Pitcrew did (start, stall restart, update, registration).
- **Disk.** A runner's page ▸ Disk shows the folder's contents, a preview of what a cleanup would delete and why, and
  *Clean now*.
- **Gitea read token** (Settings ▸ Gitea tokens, scope `read:repository`): adds workflow names, job outcomes and queue
  times to the Dashboard and Activity. Without it everything else works.
- **Diagnostics** (Settings ▸ Diagnostics): a redacted zip for support. It contains no tokens.
- **Quitting the app** never stops the runners. To stop supervising, stop the runners or remove the service
  (Settings ▸ Service ▸ Uninstall); the runner folders stay.

## Keeping disks safe, and quiet hours

- **Settings ▸ Defaults ▸ Disk guard:** below the drain threshold Pitcrew pauses runners after their current job and
  resumes them by itself when space is back. Resume a runner yourself to override it until the disk has recovered.
- **Settings ▸ Defaults ▸ Pause windows:** no jobs during a schedule, or only while the machine is otherwise idle.
- **Storage** (sidebar): the container engine's use, a preview-then-confirm prune, and each runner's Actions cache.
- **Insights** (sidebar): slow and flaky jobs, queue time and success rates per repository, with advice. Needs a Gitea read
  token for outcomes and waits.
- **Notification channels:** Settings ▸ Notification channels (webhook, ntfy, email).
- **Open in Debrief:** the Debrief button next to a job in Activity or on the Logs page.

## Changing a runner's configuration

Runner page ▸ **Configuration**. Change a setting in the form (or switch to **Advanced** to edit the YAML), then **Review
changes**: the dialog shows the diff, says a backup of the current file is made and whether the runner restarts. **Save and
apply** writes the file; a running runner restarts after its current job. Comments and layout in your file are kept. Backups
are `config.yaml.bak-<date>-<time>` next to the file (the newest 10 are kept); to undo, copy one back over `config.yaml`.
After the restart the tab says whether the runner declared the labels from the file. If the form says it cannot edit a
key safely (for example a block scalar), use Advanced.

## Processes and leftovers

Runner page ▸ **Processes** lists what the runner started. A **leftover** (a process of a finished job that is still
running) is stopped automatically 10 minutes after its job ended, but only while the runner is idle. Use **Keep longer**
for one process or Stop now (asks for confirmation). Settings ▸ Defaults ▸ *Leftover processes* turns the cleanup off,
changes the default time and edits the rules; a runner's page can override them. Examples:

```toml
[defaults.processes]
cleanup = true                              # false: leftovers are listed, never stopped
keep = { amount = 10, unit = "minutes" }    # minutes, hours or days
stop_timeout_secs = 15                      # asked to stop, ended after this long

[[defaults.processes.rules]]                # first match wins; * and ? ; name, path or command line
pattern = "*GradleDaemon*"
keep = { amount = 3, unit = "hours" }

[[defaults.processes.rules]]
pattern = "adb"
keep = "never"

[runners.processes]                         # per runner: cleanup, keep and rules (tried first)
cleanup = false
```

The shipped rules are Gradle and Kotlin daemons 3 h, QEMU and the Android emulator 30 min, adb never.

## Updates

- **Runners.** Per runner (page ▸ Version and Update policy) and as defaults (Updates page ▸ Default schedule): *Automatic*
  (inside an optional window), *Notify only* or *Manual*; the channel (stable or pre-release); **Pin** a version (also how
  you go back) or **Skip** one; **Check now** and **Update now**. An update waits for the job, swaps the binary and
  rolls back if the server does not accept the new one; the result stays on the page. A binary you provide
  (`binary_path`) is never updated.
- **Pitcrew itself.** Updates ▸ Pitcrew ▸ *Check for updates*, then *Install and restart*. Checks happen only when you ask;
  installers are signed, and a build without the signing key says updates are not available. The service and the runners
  keep running; the service restarts itself when its program changed and nothing is running.

## Configuration file

Everything in Settings is saved to `pitcrew.toml` in the data folder (validated by the service like a file it loads).
You can edit it by hand; the service reads it again when it is saved through the app, and `pitcrew-service check-config`
prints the effective configuration and its problems. The schema is documented in `crates/pitcrew-core/src/config.rs`.

```toml
schema_version = 1

[defaults.watchdog]        # the stall rule
threshold = 6
window_secs = 900

[[runners]]
name = "build-linux"
kind = "gitea-runner"
dir = "/home/ci/.local/share/Pitcrew/runners/build-linux"
labels = ["ubuntu-latest:host"]

[runners.env]              # macOS and Linux; at most 32 variables; PITCREW_* and GITEA_RUNNER_* are reserved
NODE_ENV = "ci"
```

Per runner you can override `update`, `watchdog`, `cleanup` and `processes`; everything else follows `[defaults]`. A damaged file is set
aside as `pitcrew.toml.damaged-<time>` and the last good copy (`pitcrew.toml.bak`) is used; one bad runner entry is
skipped with a problem shown in Settings; a file from a newer Pitcrew is read-only.

## Files and logs

| What | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Data folder (`pitcrew.toml`, `state.json`, `logs/service.log`) | `%ProgramData%\Pitcrew` | `~/Library/Application Support/Pitcrew` | `~/.local/share/Pitcrew` |
| Runner folder and log | `<data>\runners\<name>`, `logs\runner.log` | same layout under the data folder | same layout under the data folder |
| Service manager entry | service "Pitcrew" | `net.decryptology.pitcrew.service` launch agent | `~/.config/systemd/user/pitcrew.service` |
| IPC endpoint | `\\.\pipe\pitcrew` | `pitcrew.sock` in the data folder | `$XDG_RUNTIME_DIR/pitcrew/pitcrew.sock` |

## Command line

`pitcrew-service` (next to the app, or in the bundle):

| Command | Does |
| --- | --- |
| `run` | Supervise in the foreground (console, launchd, systemd, development). |
| `install`, `uninstall` | Register or remove the service with the OS (the app does this for you). |
| `status` | Ask the running service for its status, for scripts. |
| `check-config` | Print the effective configuration and its problems. |
| `version`, `help` | |

## Troubleshooting

| Symptom | What to do |
| --- | --- |
| "Service unavailable" banner | Settings ▸ Service: install or start it. On macOS approve it under Login Items; on Linux check `systemctl --user status pitcrew.service`. A version mismatch says to install the same Pitcrew version for both parts. |
| macOS asks "Pitcrew.app would like to access files in your Documents folder" | A CI job touched a protected folder. Pitcrew now starts runners as their own responsible processes so it is no longer blamed ([macOS guide](macos.md#privacy-prompts-from-ci-jobs)); make jobs avoid protected folders. To clear an old decision: `tccutil reset SystemPolicyDocumentsFolder net.decryptology.pitcrew`. |
| Runner **NotRegistered** | The server forgot it or the token was refused; register it again. |
| Runner **Degraded** | The server or network is unreachable; Pitcrew keeps trying and restarts the runner if polls time out while it is idle. |
| Runner **CrashLoop** | Read its log; fix the cause; press Start to clear it. |
| Runner **WaitingForLogon** (Windows) | An interactive runner waits for a user to log on; choose the service session or the service account if it must run before that. |
| Jobs cannot find `node` or `cargo` (macOS, Linux) | Pitcrew passes the login shell's `PATH`; check the shell answers (`$SHELL -l -i -c 'echo $PATH'`) or set `PATH` in `[runners.env]`. |
| "A runner binary already runs" | Another supervisor (an old scheduled task, a launchd job) runs this runner; stop it. |
| An update was rolled back | The runner's page shows the reason; the previous version runs again; try again later or pin a version. |
| Tray icon missing (Linux) | GNOME needs the AppIndicator extension; with no tray, closing the window quits the app. |
