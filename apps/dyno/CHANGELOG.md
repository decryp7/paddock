# Changelog

## [1.0.0] - Unreleased

### Added

- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer (ADR 0021). On Windows, macOS and Linux, with your own rights: no privileged helper and no administrator prompt (the helper is planned for 1.1, ADR 0022), and a figure the OS keeps from you says *Unavailable* with the reason.
- A per-user background agent samples the machine and keeps its history while the window is closed; the app and the `dyno` command line talk to it over one local IPC. Its own cost is shown ("Dyno: x % CPU, y MB") and it stretches its sampling when it is over budget.
- Live dashboard: CPU, memory, disk and network, plus GPU, battery, temperatures and fans where the OS reports them (otherwise the reason, never a zero), with sparklines, a health score and customisable widgets.
- History in SQLite that survives closing the window: raw points for a day, then 1-minute, 15-minute and hourly rollups with min, average and max, so a short spike stays visible; retention is configurable and a damaged database is set aside and recreated (ADR 0008).
- Processes grouped by application, with details (command line, modules, connections, signer; macOS and Linux first), "what is locking this file", open location, snapshots and diffs, network traffic per program where the OS allows it, and the **Other users** row.
- Why is it slow: a diagnosis with causes, confidence, evidence and suggestions, anomalies, a disk-full forecast, throttling detection, SSD and battery life ranges and learned "is this normal" baselines (ADR 0018).
- Alarms with an ISA-18.2 style lifecycle (low disk, overheating, crash loops, throttling, stale backups), acknowledged, muted and annotated in Dyno's own data (ADR 0013); issues from the OS logs (crashes, hangs, unexpected shutdowns) with a stability score and export for Debrief (ADR 0017); new and unusual programs against a learned baseline (ADR 0017); and a weekly report for any of the last 52 weeks, exported as Markdown or a web page.
- Disk map: a ring chart of a volume or folder where a click drills in, with a live scan job (phases, Cancel, bounded in depth, entries and time), Retry for folders the OS refused, Reveal and Copy path, and the Full Disk Access prompt on macOS. It deletes, moves and renames nothing.
- Health and startup items with reasons only, a performance-counter browser with charts, named sets and dashboard graph widgets, extras (what changed, sleep, boot, per-app network and energy, GPU per process, large and duplicate files, inventory with memory-slot drawings and export, maintenance windows), backup health (Time Machine, Windows Backup and File History, restic, Borg), benchmarks on click, a flight recorder and the diagnostics bundle.
- Web and VirusTotal search from right-click menus only open your browser, with private details removed first; the file hash is computed on your computer and nothing is uploaded.
- The `dyno` command line (`status`, `why-slow`, `top`, `alarms`, `snapshot`, `counters`; text or `--json`), shipped in the installers.
- Out-of-process Rust sensor plugins with timeouts, a memory cap and quarantine, listed in Settings.
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, the tray with a live metric and compact dropdown, a command palette, and light and dark themes with a colour-vision-checked palette. Closing the window keeps Dyno measuring.
- Every dialog is on the shared dialog API (ADR 0026): the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
- Updates are opt-in and signed with the Paddock family key: one HTTPS GET of the `dyno-latest` release, every update verified before it installs, and a build without the key never checks. The installers are an NSIS setup (Windows, per user), an ad-hoc signed `.dmg` (macOS) and a `.deb` and AppImage (Linux), each with the `dyno-agent` and `dyno` sidecars.
