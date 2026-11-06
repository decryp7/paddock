# Changelog

## [1.1.0] - 2026-11-06

**On Windows, uninstall Dyno 1.0.0 first (keep your data), then install 1.1.0; 1.0.0 may offer this update but cannot install it.**

To keep your data, open Settings > Apps, uninstall Dyno and leave "Delete the application data" unchecked. Then run the 1.1.0 setup from the releases page. Your history stays. On macOS and Linux, update as usual.

### New

- Administrator helper: optional. Turn it on, repair it, update it or turn it off in Settings.
  - With it, Dyno reads every account's processes, temperatures, fans, energy, memory modules and which program owns each connection or file.
  - Without it, everything keeps working with your own rights. A reading Dyno cannot get says why.
  - Dyno asks the system for permission each time the helper is installed, repaired or removed.
- Power actions: Sleep, Hibernate and Shut down by a rule. Rules can follow idle time or a time of day.
  - A countdown with Cancel shows first. Snooze delays the action.
  - Keep awake holds the computer awake for a time, until a time of day, until you stop it, while a program runs, or while the computer is busy. It is on the Power page, in the tray menu and in the command "dyno awake".
- Wake-on-LAN: save your other computers, wake them and test whether they wake. A check of this computer says whether it can be woken over the network and what to change.
- Scheduled wake: wake this computer at a time you choose, keep it awake for a while, and wake chosen computers. It is off until you turn it on in Settings. A wake that was missed is shown as missed and is never run late. Nothing can wake a computer that is shut down.
- AI access over MCP: Settings > AI access offers read-only tools to a program on this computer that speaks MCP, such as Pitwall.
  - It is off until you turn it on. Nothing listens until it is on and you have made a key.
  - Every call is listed on the page. Every tool has its own switch.
  - Keep awake and wake a computer can change things. Each needs your permission by name.
  - Dyno never gives out command lines, user names, program paths, the computer's name, network addresses or alarm details.
- Jobs view: changing Start at login shows Cancel while it works. Cancel stops the change at its next step.
- Windows: Dyno installs once for every user of the PC, in Program Files.
  - Updates from the app show one Windows permission prompt. A standard user types an administrator's name and password.
  - If you decline the prompt or the installer stops, Dyno starts again and says so, with Install again.
  - After an update the helper is refreshed if it is on. If that fails, Settings offers "Update the helper".
- Saved views: disk maps, the counter list and the installed-software list show at once after a restart, with their age.
- Benchmarks: a processor, memory and disk test that you start yourself. It repeats, shows the middle result with its spread and marks an unsteady result. The disk test writes at most 14.5 GiB per drive and deletes it at once. On battery it asks first.
- Network: choose lists of addresses and Dyno marks matching connections. See your connections on a world map and read your firewall rules. Dyno never changes the firewall. Each network feature has its own switch.
- Every metric can be its own menu-bar or tray item, up to 32.
- The dashboard is a free-size grid of widgets. A 1.0 dashboard is upgraded on the first start.
- Report a problem is one dialog. It explains a broken helper, how Dyno starts at login and what is not in the report. It warns that issue reports are public.
- If a window does not start, you can still save a problem report from the question Dyno asks, or from a terminal with the option "--export-diagnostics".
- Quit and stop measuring closes Dyno and the agent until you open Dyno again or log in.
- Notifications: select and delete messages, or delete all. Deleted messages stay gone after a restart.
- Dialogs fit their content and keep the size you give them.

### Fixed

- Repair of the login service no longer waits behind a start, and says what happened and what to do next. Quit waits for a repair or cancels it. Dyno no longer starts a second agent while the system is still registering the service.
- Start at login: turning it off stays off. On Linux, a unit you disabled stays disabled.
- macOS: Dyno no longer asks for your Keychain password after an update. Wake-on-LAN passwords and AI access keys are now kept in files only you can read.
- Snooze on a timed power action now delays it. Before, it cancelled the action for the day.
- The window and tray no longer miss updates from the agent when they are busy (power, scheduled wakes, alarms, alerts, activity).
- A scheduled wake saved while Dyno was syncing wakes is no longer cancelled.
- Dyno lowers its background threads on Linux even when started under "nice".
- An update package is checked again right before it is installed.
- macOS: memory in use matches Activity Monitor. Memory alarms can now fire on a Mac where they stayed quiet.
- Lifespan on an Apple silicon Mac reads the internal SSD.
- Health: an active alarm lowers the health score.
- Disk map: one folder that times out no longer stops the whole scan.
- Windows: at logon the tray icon and window start, and no command prompt stays open. The helper starts with the computer, and says why when it cannot.
- Windows firewall rules show the name Windows shows.
- Tables grow and shrink with the window. The page scrolls once.
- Reports: the Disks card no longer spills, and tiny or empty volumes are left out.
- History: switching the range no longer leaves the chart empty.
- macOS: menu-bar icons no longer flash, and show their reading when you hover.
- "Is this normal?" counts days by your own calendar.
- About: "See all releases" opens the releases page.

### Removed

- Backup health: the page, its Settings section, the widget, the alarms and the "backup" command. Saved settings that name it still load; the backup parts are dropped.

### Changed

- Lists in widgets show everything and scroll.
- Lower CPU use when no window is open.
- The Processes tree opens quickly with thousands of processes.
- The log says what you did (not what you typed) and how the last run ended.

## [1.0.0] - 2026-10-29

### Fixed

- macOS: a look that waits for the login service (`SMAppService.status`, 21 s on a loaded login) reads the discovery file and the lock again every 100 ms and returns as soon as an agent holds the lock, so the app connects when launchd's agent is up instead of when the service answers.
- The diagnostics bundle reads its logs from the folder `run` named (carried by the agent's folders), so a test harness never copies the machine's real Dyno logs; a log that does not fit is left out instead of failing the save, and the newest logs are taken per process.
- Report a problem is reachable: Help menu item and a command-palette entry (the UI never imported the shared diagnostics dialog); it shows the app log and the agent log read from disk, so both are there with the agent stopped. The Diagnostics page's saved bundle now holds the newest app and agent logs (`logs/`).
- macOS: a service recheck in the wait for the agent no longer blocks the look while the folder lock is held (the agent is up): at login under load the rechecks, each waiting on `SMAppService.status`, delayed the connect.
- macOS: a restart with Dyno running ("Reopen windows when logging back in") starts Dyno hidden in the tray, whatever Start at login says; loginwindow's restore request (`rsto`, sender checked by its executable path) counts like a login-item start (`lgit`). The log says which signal decided (`flag`, `login item`, `restored at login`).
- Settings: when a save cannot reach the agent ("The Dyno agent is not running"), the page puts the controls back to what is stored and shows Retry, which sends the edit again. Before, the switch kept the unsaved position, so the next press equalled the stored value and nothing was saved. An engine refusal still keeps the edit so it can be corrected.
- macOS: at login the app connects to the agent that launchd started as soon as its file is there, without waiting for `SMAppService.status`. That call is an xpc round trip to smd and blocked the first look for about 30 s on a busy login (sampled in the VM); the service is now asked again right after the connection, for the hand-over, and still decides a look that has no agent to connect to.
- An agent that the system or another app (App Tamer) has stopped is reported as "Paused by macOS or another app" and waited for: no Retry on the chip, and nothing ends or replaces it (ADR 0031). Before, a stopped start counted as having done nothing for 60 s and Retry ended it.

- Logs for root cause (login start and every other start): the agent now writes its own dated log (`dyno-agent.<day>.log` beside the app's `dyno.<day>.log`, with the panic hook and a crash marker) with a line per step of its start (lock, store, endpoint bound and `agent.json` written, running, stopping) and millisecond counts; its errors go there instead of standard error. The app's own lines go to the same shared file as the shell's (the separate `logs/app.log` is gone) and tell the login start step by step: the open event that started it, each decision look with the service's answer and how long `SMAppService.status` took, the discovery file and lock, every change of the link's phase with its duration, connect attempts, launches, closes and failed requests (names and codes only), and the time from connecting to the first data. Repeats are told once. Report a problem now includes both logs and the agent's own `agent.log`, and works with the agent down.
- Fixed a gap that skipping the service on the decision look opened: switching Start at login off right after the app connected, before the service had answered once, no longer leaves the service's agent connected and unsupervised (a change of the service with no earlier answer takes the agent back; a first answer that is only a baseline still does not).
- Logs: the agent's refusal says why, who holds the folder (its pid) and what started the refused agent, and `agent.log` is gone (everything is in the dated log); connect attempts carry the attempt number, the time since the link began and the attempts a success took; a connect in the app's log and the agent's lines share `AgentRun`; a flapping link is summarised once a minute; failed requests are told once per request name.
- The first look at login no longer waits for `SMAppService.status` when an agent is already there: the look was wrongly never marked as a decision look, so the service was always asked first.
- macOS: a healthy agent that launchd runs is no longer thrown away at login. The reader of `launchctl print` took the nested `state = active` lines of the coalitions for the job's state, so a running job looked stopped, and "has a launch constraint" alone counted as refused; the repair then killed the agent four times and printed "needs LWCR update" without launchd having said so. Only spawn failed, exit 78 and "needs LWCR update" mark a refused job, a running job is never replaced because the app has not connected yet, and the failure message prints the state launchd reported. `launchctl` output is read while it runs (a listing over 64 KiB blocked it until the 5-second timeout on every look).
- macOS: the reader of `launchctl`, the "is this job refused" decision and `launchctl bootout` now come from the shared `paddock-service` crate (no copy in Dyno); a pending or running job is never called stale, and `launchctl` output is read while the tool runs.
- The agent's `settings.json` is no longer moved aside as `settings.json.corrupt` at every app start (the shell used the same file name; see the shared changelog), so Start at login and the other settings survive a restart.
- macOS: the Network page, a program's connections and open files, and "what is locking this file" no longer ask the kernel about other programs' sockets, pipes and kqueues one descriptor at a time (`proc_pidfdinfo`, `lsof`), which could rarely make macOS fail another app's waiting reads and writes on a socket with EBADF. Sockets now come from the kernel's own TCP and UDP lists (the owner is the process the socket works for), files and libraries from `libproc` file descriptors only (Dyno ADR 0026). The agent and the `dyno` command carry a dotted code-signing identifier (`net.decryptology.dyno.agent`, `.cli`) from an embedded Info.plist, which macOS 27 needs to show the whole list; without it the Network page says why instead of listing only Dyno's own sockets.
- macOS: turning on Launch at login adds Dyno's own two entries to Login Items, the app and its agent (the app registers itself with `SMAppService.mainApp`; no extra "dyno-app, unidentified developer" row, nothing under `~/Library/LaunchAgents`), it is refused with a message when Dyno runs from the disk image or a quarantined download, and a login start still opens in the tray. `dyno-agent service` refuses inside the app on macOS. The "First launch" notes describe the macOS 26 steps.

### Added

- Widget layouts are shared with the other apps (family ADR 0035, `paddock-widgets`): the tray popup, the dashboard and the menu-bar items each have a closed catalogue (`dyno_core::widgets`), live in the agent (`settings.json` `popover`, `dashboard`, `menuBar`) and change only through `EditWidgetLayout`, which validates the whole result, saves it and tells every window. The popup shows the widgets you choose (default: active alarms, disk space, CPU and memory; up to 8), each with its own loading, empty, error and working state, and says "Agent stopped" once, with Restart agent, when the agent is away. Values are built in one pure module (`widgets::popover_values`) from what the agent already keeps and the live push, and are sent to the popup only while it is visible. `PROTOCOL_VERSION` is 3. Pinned counters are not in the live push: while the popup is visible the app asks the agent for them every 5 seconds (from the counters' kept readings, never a fresh OS read); the backup widget carries `lastBackup` and the UI draws its age. The old `tray` and `trayItems` settings keys are gone (Dyno 1.0 is unreleased: nothing is migrated). The default popup fits the family's 520 px popup cap without scrolling, also while the agent is stopped: one alarm row and two volume rows are shown before "+N more" (`tray-fit.spec.ts` checks the default and the taller states).
- The widget UI is on the shared components (family ADR 0035): the tray popup draws the engine's layout with `pd-widget-area` and one template per widget kind (the calm alarms row, the agent notice, the ⋯ footer menu with Customize widgets…, Settings… and Quit Dyno; a shortcut is shown only if the popover window binds it, as Ctrl on Windows and Linux); Settings > Menu bar has the popup widgets chooser with a live preview and the menu-bar items editor, and the dashboard's edit mode is the same chooser with presets (edits apply at once and have Undo; there is no Save dashboard). The TypeScript copy of the layout model (`widget-model.ts`) and Dyno's hand-written `Settings`, `DashboardLayout` and `TrayItems` types are gone: only the generated types are used. The dashboard no longer colours a widget by threshold until the engine supplies dashboard values.
- The link to the agent is now a state machine with one owner (ADR 0025): a request made before the first connection waits for the connection or for a shown stall with Retry instead of failing after 8 seconds, a connection that drops right after it was made backs off, and a start, a hand-over and a quit can no longer interleave into two agents or a wrong "stopped" note.
- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer. On Windows, macOS and Linux, with your own rights: no privileged helper and no administrator prompt (the helper is planned for 1.1), and a figure the OS keeps from you says *Unavailable* with the reason.
- A per-user background agent samples the machine and keeps its history while the window is closed; the app and the `dyno` command line talk to it over one local IPC. Only one agent runs per user: it holds a lock on its data folder, a second one exits at once (code 3) without touching the history, and the app waits for a slow-starting agent instead of starting another one (a cold, slow disk can take a minute), then stops exactly the agent it started when you quit. Turning Launch at login on hands the agent over to the system's login service (macOS, Linux) instead of running two; turning it off brings back the app's own. A start that takes long is never given up on, but the status bar says why (still starting, waiting for the login service, another agent holds the data folder) with a Retry; if the login service starts no agent, Dyno starts its own and says so. Its own cost is shown ("Dyno: x % CPU, y MB") and it stretches its sampling when it is over budget.
- Live dashboard: CPU, memory, disk and network, plus GPU, battery, temperatures and fans where the OS reports them (otherwise the reason, never a zero), with sparklines, a health score and customisable widgets.
- History in SQLite that survives closing the window: raw points for a day, then 1-minute, 15-minute and hourly rollups with min, average and max, so a short spike stays visible; retention is configurable and a damaged database is set aside and recreated (one another process is writing is not damaged and stays).
- Processes grouped by application, with details (command line, modules, connections, signer; macOS and Linux first), "what is locking this file", open location, snapshots and diffs, network traffic per program where the OS allows it, and the **Other users** row.
- Why is it slow: a diagnosis with causes, confidence, evidence and suggestions, anomalies, a disk-full forecast, throttling detection, SSD and battery life ranges and learned "is this normal" baselines.
- Alarms with an ISA-18.2 style lifecycle (low disk, overheating, crash loops, throttling, stale backups), acknowledged, muted and annotated in Dyno's own data; issues from the OS logs (crashes, hangs, unexpected shutdowns) with a stability score and export for Debrief; new and unusual programs against a learned baseline; and a weekly report for any of the last 52 weeks, exported as Markdown or a web page.
- Disk map: a ring chart of a volume or folder where a click drills in, with a live scan job (phases, Cancel, bounded in depth, entries and time), Retry for folders the OS refused, Reveal and Copy path, and a hint while Full Disk Access is missing on macOS. It deletes, moves and renames nothing.
- Health and startup items with reasons only, a performance-counter browser with charts, named sets and dashboard graph widgets, extras (what changed, sleep, boot, per-app network and energy, GPU per process, large and duplicate files, inventory with memory-slot drawings and export, maintenance windows), backup health (Time Machine, Windows Backup and File History, restic, Borg), benchmarks on click, a flight recorder and the diagnostics bundle.
- Web and VirusTotal search from right-click menus only open your browser, with private details removed first; the file hash is computed on your computer and nothing is uploaded.
- The `dyno` command line (`status`, `why-slow`, `top`, `alarms`, `snapshot`, `counters`; text or `--json`), shipped in the installers.
- Out-of-process Rust sensor plugins with timeouts, a memory cap and quarantine, listed in Settings.
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, the tray with a live metric and compact dropdown, and light and dark themes with a colour-vision-checked palette. Closing the window keeps Dyno measuring.
- Every dialog is on the shared dialog API: the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
- About shows the build's commit and date.
- Updates are opt-in and signed with the Paddock family key: one HTTPS GET of the `dyno-latest` release, every update verified before it installs, and a build without the key never checks. The installers are an NSIS setup (Windows, per user), an ad-hoc signed `.dmg` (macOS) and a `.deb` and AppImage (Linux), each with the `dyno-agent` and `dyno` sidecars.
- Settings > Parallelism and cost: how many threads read folders (Auto or 1 to 16, for the disk map and the file finder; Auto follows the kind of volume), how many jobs run at once (Auto or 1 to 8, read at start) and the file finder's limits (files looked at, folder depth, content compared), all validated by the engine. The finder skips a file over the comparison budget and carries on, counts the folders it could not read, and counts the threads of a cancelled search on a dead share together with the disk map's per host (restarting Dyno clears it). Child processes of the log read are the shared `paddock-proc`: their own process group ended as a whole, lower priority without `nice(1)`.
- Finding large and duplicate files, reading the system logs, checking the backups, hashing a program's file and the sleep, boot, app-usage and software reads are jobs on the shared `paddock-jobs`: starting one answers at once, the agent pushes every status to the shell (`paddock://jobs`), so the Jobs view, the status item and the pages show it with the phase's own count (a growing count while it walks or streams, then the comparison, then the bytes read), "Still working" at the shell's threshold, and Cancel in every phase. A cancelled or failed job changes nothing, and no timeout of the UI ever abandons a running one (the UI has no timer at all; a shared check over the app's sources keeps it so). Cancelling a system-log read kills the `log show` process at once; a log read or backup check asked for while the scheduled one runs waits for it. The file finder is bounded in files (500,000), folder depth (48 levels) and bytes compared (64 GiB) and says which limit stopped it and what to do. The agent protocol is version 2 (`StartJob`, `Jobs`, `CancelJob`, `JobResult` replace `FindFiles`, `RescanIssues`, `RefreshBackup`, `FileHash`, `SleepBlockers`, `BootAnalysis`, `AppUsage` and `SoftwareInventory`; `Message::Job` pushes a status).
- A command palette: Mod+P goes to any page, Mod+Shift+P runs a command (the pages' own buttons, such as Read the system logs now, Check now, Run again, Save bundle, Add rule; each shows its page first and is off while the page's button is).
