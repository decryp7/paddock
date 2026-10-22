# Features

What Dyno does, and honestly how far each feature is. **Built** means the code exists and is tested; **Engine only** means
the agent and core implement it and the protocol exposes it but the UI screen is still a placeholder; **Planned** means it is
not in the code (see [roadmap.md](roadmap.md) for the work package). Screenshots are in `docs/screenshots` (dark and light).

## Contents

- [Live monitoring](#live-monitoring)
- [History](#history)
- [Processes](#processes)
- [Why is it slow?](#why-is-it-slow)
- [CPU taming and profiles](#cpu-taming-and-profiles)
- [Health and startup](#health-and-startup)
- [Cleanup](#cleanup)
- [Tuning](#tuning)
- [Alerts and reports](#alerts-and-reports)
- [Tray, window and settings](#tray-window-and-settings)
- [Safety](#safety)
- [Extensibility](#extensibility)
- [Planned features](#planned-features)
- [Platforms](#platforms)

## Live monitoring

**Built.** The agent samples CPU (total, per core), memory (used, commit, pressure), disks (free space, throughput),
network, and GPU, battery, temperatures and fans where the OS tells; every sensor that is missing shows *why* (a `Reading`
with a reason), never a zero. Intervals adapt: 1 s and 2 s by default while someone watches, 2 s and 10 s when nobody does,
doubled on battery, stretched when Dyno itself is over its CPU budget. The dashboard shows tiles with sparklines, the top
apps by CPU and memory, and "Dyno: x % CPU, y MB" (its own overhead, measured; [architecture](architecture.md#dynos-own-overhead)).

![Dashboard](screenshots/dashboard-dark.png)

## History

**Built.** The agent keeps raw points for a day, then 1-minute, 15-minute and hourly rollups with min, average and max (so a
short spike stays visible at every zoom level), per-app minutes for "top consumers", disk space for the forecast, timeline
events and boot records, in SQLite ([ADR 0008](adr/0008-storage-sqlite-and-retention.md)). History keeps running with the
window closed. Retention is configurable. A damaged database is moved aside and recreated. The **crashes** series is empty
until Issues is built ([roadmap G4](roadmap.md#known-gaps-in-the-foundation)).

## Processes

**Built.** A tree grouped by application ([ADR 0003](adr/0003-process-grouping.md)); details per process (command line,
working directory, modules, connections, signer; on macOS and Linux today, Windows details are not wired yet); "what is locking
this file" (`lsof`, `/proc`; Windows not wired); end, suspend, resume, priority, affinity (not on macOS), efficiency mode and
CPU cap for a process or a whole app; open its folder. Every action is checked against the protected list, names the process by
id and start time, is logged in **Activity** and can be undone. Protected processes show a lock and the reason. Snapshots of the
process list can be taken and compared.

![Processes](screenshots/processes-dark.png)


**Network traffic per program** (Network page, process details): macOS from `nettop`, Linux from the kernel's per-socket TCP byte
counters (no root; other users' programs are not attributed), Windows needs administrator rights for the OS counters and says so
until the helper does it. Where it cannot be read the page shows the reason instead of zeros.

## Why is it slow?

**Engine only.** `WhySlow` returns a diagnosis: a headline and causes (kind, confidence, evidence, suggestions such as "lower
priority of X"); `Anomalies` lists unusual behaviour; disk-full forecasts come from history. The dashboard shows the diagnosis on
request ("Check now"). The dedicated Why slow screen is a placeholder.

## CPU taming and profiles

**Engine only.** Rules with a condition (an app's CPU above a threshold for a time, optional context such as user idle or on
battery) and an escalation ladder (lower priority, efficiency mode, a hard cap, notify) are evaluated by the pure `Engine` and
carried out per OS (Windows job objects; a 100 ms SIGSTOP/SIGCONT duty cycle on macOS and Linux; EcoQoS and background QoS;
`nice`), released when the app calms down, when the rule stops applying and when Dyno stops, also after a crash
([ADR 0004](adr/0004-cpu-taming-and-throttling-methods.md)). Profiles (Normal, Gaming, Meeting, On battery, Building code)
select the active rules. **Rules ship disabled.** The CPU taming screen is a placeholder; rules are managed through the
protocol only (`Rules`, `SaveRules`, `SetProfile`, `PauseRules`). The protected list is absolute and rules never touch the
foreground app.

## Health and startup

**Engine only** (the dashboard shows the health score and fixes). `Health` returns a score and grade with reasons and fixes;
each fix explains and previews itself before anything changes. `StartupEntries` lists what starts at login (Windows: not
wired yet; macOS: launch agents and login items; Linux: autostart and user units); `SetStartupState` enables or disables an
entry that needs no administrator rights, undoable. The Health screen shows the score, its reasons and the startup manager.

## Tuning

**Built** (engine and screen). A catalogue of reversible suggestions that change something only on a click: power plan or mode
(Windows plan, Linux profile, macOS Low Power Mode), sleep timer on battery, hibernation, battery charge limit where Linux exposes
it, search indexing exclusions for developer folders, Defender exclusions for build folders, visual effects and background apps.
Each previews its exact change and its exact inverse; applying records an undoable activity entry and reads the setting back to
confirm it took effect; before a revert the setting is read again and a change made outside Dyno since is detected (Revert
anyway overwrites it on purpose). The changes are a closed set of typed actions behind a port with a fake for tests; nothing
runs text as a command. Changes that need administrator rights (Defender, hibernation, charge limit, macOS sleep and Low Power
Mode) are listed with the reason and are not applicable until the privileged helper exists ([WP-01](roadmap.md#wp-01-privileged-helper)).
Really applicable now: Windows power plan, sleep timer, visual effects, background apps and indexing; Linux power profile;
macOS indexing exclusions (Spotlight markers). Windows commands are unit-tested through a fake runner, not yet run on Windows.

## Cleanup

**Policy built, scanner and deleter planned.** The catalogue, the admission policy (never links, never personal folders, never
outside the category's roots, age filters, preview before delete, scan validity of one hour) and per-OS root resolution exist and
are tested ([ADR 0005](adr/0005-cleanup-safety.md)). The agent answers cleanup requests with "not built yet"; the disk-space map
too. Linux has no categories yet. Work package: [WP-04](roadmap.md#wp-04-cleanup-with-quarantine-and-undo).

## Tuning

**Planned.** The core has the plan and `TuningSuggestion` types and the `HelperAction` set; the agent answers "Tuning is not
built yet." Work packages [WP-01](roadmap.md#wp-01-privileged-helper), [WP-05](roadmap.md#wp-05-tuning-catalogue).

## Alerts and reports

**Engine only / planned.** Alerts (low disk, overheating, crash loops, rule actions) are stored and listed (`Alerts`); the weekly
report type exists but the agent answers "The weekly report is not built yet." The Reports screen is a placeholder.
[WP-08](roadmap.md#wp-08-alerts-and-the-weekly-report).

## Tray, window and settings

**Built.** A tray icon with a live metric (CPU, memory, …, chosen in Settings; a template image on macOS) and a compact dropdown
window; closing the window destroys it and keeps the tray and the agent; Quit exits the app and leaves the agent measuring. A
command palette (Ctrl or Cmd + K). Settings: theme (system, dark, light), tray metric and style, sampling intervals, history
retention, launch at login, alert settings. Light and dark themes with a colour-vision-checked palette
([ADR 0011](adr/0011-design-system.md)); a desktop feel ([ADR 0009](adr/0009-native-feel.md)): no text selection on chrome,
Dyno's own Copy menu, no browser keys.

![Settings](screenshots/settings-dark.png)

## Safety

**Built.** Observe-only by default; rules disabled; the protected list (critical OS processes, antivirus, audio engine, Dyno,
your entries) cannot be overridden; actions name a `ProcessRef`; every action is logged and undoable; the agent never runs
elevated; tests act only on processes and folders they create. The privileged helper for system changes is planned
([ADR 0006](adr/0006-privileged-helper-and-observe-only.md)).

## Extensibility

**Partly built.** Out-of-process Rust plugins (`dyno-plugin-api`, `dyno-plugin-host`, an example plugin; guarded with timeouts,
a memory cap and quarantine) are built and listed in Settings; their contributions are minimally wired ([G5](roadmap.md#known-gaps-in-the-foundation)).
Declarative TOML packs are planned ([WP-15](roadmap.md#wp-15-declarative-toml-packs)). See [plugins.md](plugins.md) and
[ADR 0007](adr/0007-extensibility-three-tiers.md).

## Planned features

All in [roadmap.md](roadmap.md): Issues with statistics, new and unusual processes, right-click search, watched folders with
keep-for times, benchmarks on click, a performance-counter browser, the tuning catalogue, extras (what changed, why won't it
sleep, boot analysis, network and energy per app, inventory report, GPU per process, large and duplicate file finder,
maintenance windows), backup health checks, declarative packs, a local web API, "Explain this" via Pitwall, a multi-PC LAN view
later.

## Platforms

| | Windows 10 / 11 | macOS 13+ | Linux (systemd) |
| --- | --- | --- | --- |
| Sensors | CPU, memory, disk, network through performance counters | sysinfo plus memory pressure, GPU | sysinfo plus PSI, diskstats, hwmon, GPU |
| Process control | priority, affinity, EcoQoS, suspend, job-object CPU caps | signals, background policy, duty-cycle caps | signals, nice (per thread), affinity, duty-cycle caps |
| Details, locks, startup, facts | not wired yet | built | built |
| Agent at login | per-user Run entry | launch agent (`SMAppService`) | systemd user unit |
| Installer | NSIS | `.dmg` (ad-hoc signed) | `.deb`, AppImage |
