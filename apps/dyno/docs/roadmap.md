# Dyno roadmap

What is built is in [features.md](features.md); this page is everything that is not. Work packages (WP) are written so an
agent or contributor can pick one up: scope, acceptance criteria, dependencies and the files it touches. Shared integration
points (`lib.rs`, `protocol.rs`, `ports.rs`, the workspace `Cargo.toml`, `api.types.ts`) are changed by the integrating
agent only (AGENTS.md "Working in parallel"). Every package ships tests, and every robustness behaviour has a test that
triggers the failure.

Decisions already taken for these packages (2026-10-22): three-tier extensibility ([ADR 0007](adr/0007-extensibility-three-tiers.md)),
SQLite ([0008](adr/0008-storage-sqlite-and-retention.md)), observe-only by default with a helper installed on first admin
use ([0006](adr/0006-privileged-helper-and-observe-only.md)), Linux a first-class platform next to Windows and macOS.

## Scope: v1 (planned) and Later

**v1 is planned, not frozen**: it is every work package in the "v1" table below, and the list may still grow by the user's
decision. There is no cut-down release; v1 ships when all of them are done. Other new ideas go to [Later](#after-v1).

Status words are honest: **built** (code and tests committed), **in progress** (another agent is working on it; not all of it
is committed, re-check `git log`), **planned** (nothing in the code).

| WP | Feature | Status (2026-10-23) |
| --- | --- | --- |
| WP-01 | Privileged helper | built (feat/wp01-helper): `dyno-helper` daemon, authenticated endpoint, audit log, per-OS install/removal plans, agent link and wrappers (administrator tuning, all-users startup items, Windows traffic, Defender exclusions), Settings > Helper and the install dialog ([ADR 0016](adr/0016-privileged-helper-install-and-ipc.md)). Real installs on each OS are checked by hand (testing-manual H1-H15); macOS uses the password dialog, not SMAppService, until a Developer ID exists |
| WP-02 | CPU taming screen and profiles | built: rules table, profiles, in-effect list with release, protected list, activity with undo |
| WP-03 | Health score and startup manager screen | built: Health page with reasons, fixes and startup manager (switch, delay, undo); machine-wide entries go through the helper (WP-01) |
| WP-05 | Tuning catalogue | built: catalogue, previews with inverse, undo with outside-change detection, Tuning page; administrator items go through the helper (WP-01); startup trimming stays on the Health page |
| WP-04 | Cleanup with quarantine and undo | built: scanner, re-checking deleter, quarantine with restore, Cleanup page, disk map engine, TOML packs, Linux; scheduled cleanups, Docker removal and a disk-map screen planned |
| WP-06 | Issues with statistics | built: macOS crash reports and unified log, Windows Event Log (`wevtutil`), Linux journal and `coredumpctl`; stored in SQLite; stability score, leaderboards, trend, JSON-lines export for Debrief; crashes chart, health reason and weekly report filled (G4); crash-loop alert and serious-issue alarms. Linux and Windows readers are verified by fixtures only (see Status) |
| WP-07 | New and unusual processes | built: baseline with silent learning, scoring (first seen, location, system-name lookalikes, signature, many children or connections, new startup entry), "New and unusual" view with protect, hide and restore. Windows signatures wait for G8 |
| WP-08 | Alerts and the weekly report | built: weekly report from stored data (health trend, apps, disk forecast, issues, changes), Reports page with week selector, Markdown/HTML export; Why slow page (G10). Planned: alert history on the page, weekly OS notification (superseded in part by WP-26) |
| WP-09 | Right-click search | planned |
| WP-10 | Watched folders with keep-for times | planned |
| WP-11 | Benchmarks on click | built: CPU, memory and disk (GPU reported unavailable), out of process with a timeout, load stored, before/after comparison, results page (ADR 0015) |
| WP-12 | Performance counters browser | planned |
| WP-37 | Network: open ports and connections per process, Look up IP | built: listeners, connections, filters, grouping, new-listener/host activity entries, browser lookup (ADR 0015); reverse DNS on request, per-program traffic (macOS `nettop`; Linux `sock_diag` byte counters + `/proc/<pid>/fd`, no admin; Windows per-connection TCP statistics, which need administrator rights, so it says so until the helper), row context menu, process-details Network tab, alarms for new exposed listeners and new remote hosts |
| WP-13 | Extras (what changed, sleep, boot, network and energy, inventory, GPU per process, file finder, maintenance windows) | built (feat/wp13-14-36): the Extras page with eight tabs over pure parsers with fixtures ([ADR 0019](adr/0019-extras-backup-checks-and-taming-proof.md)). Verified by fixtures only: the Windows and Linux readers. Not built: per-app network and energy are not stored as app-minute columns (shown live, macOS only for energy), and Linux GPU per process needs NVIDIA's tool |
| WP-14 | Backup health checks | built (feat/wp13-14-36): Time Machine, Windows Backup, File History, restic and Borg judged against a stale limit, alarm per tool, login-shell `PATH` resolver (G11). Duplicati is detected but not readable yet; the detectors are built in until WP-15's packs exist; Windows event IDs are fixture-verified only |
| WP-15 | Declarative TOML packs | planned |
| WP-16 | Optional local web API | planned |
| WP-17 | Explain this via Pitwall | planned |
| WP-19 | Per-core CPU view | built: P/E kinds, per-core frequency and rollups, tile toggle, detail heatmap (tray per-core bars via tray items); Windows kinds CI-only |
| WP-20 | Memory modules inventory with slot drawings | built: parsers (Windows/macOS/Linux DMI), hints, inventory page with slot drawing; Linux helper read planned |
| WP-21 | Customisable widgets (ADR 0012) | built: model, dashboard edit mode/presets, menu-bar items editor; preset save button planned |
| WP-22 | Native tray popover | built: popover.rs + dy-popover; Windows/Linux look CI/manual-only |
| WP-23 | Throttling detector | planned |
| WP-24 | Disk and battery life forecasts | planned |
| WP-25 | "Is this normal?" learned baselines | planned |
| WP-26 | Alarms view and custom alerts (ADR 0013) | built: Alarms page (active, history, rules), custom alert rules evaluated by the agent, desktop notifications (tauri-plugin-notification), quiet hours |
| WP-27 | `dyno` CLI (ADR 0014) | planned |
| WP-28 | Flight recorder | built: ring, capture, alarm capture and the timeline page |
| WP-29 | Diagnostics bundle | built: redaction (IPv4 and IPv6), preview, zip, a redacted sockets file, the page and the OS save dialog |
| WP-30 | Suite tie-ins (Spotter, Debrief, Pitcrew, Pitwall) | planned |
| WP-31 | Session recording to CSV/Parquet with a built-in viewer | planned |
| WP-33 | Right-click "search this file's hash on VirusTotal" (opens the browser only; nothing is uploaded) | planned |
| WP-34 | Startup manager filter: hide OS and signed entries | planned |
| WP-35 | Network change events (Wi-Fi joins, VPN up/down, IP changes) on the timeline | planned |
| WP-36 | Taming acceptance rules: temporary, foreground-aware, self-restoring, with measured proof | built (feat/wp13-14-36): `expiresAt` on rules, expiry switches the rule off and releases, foreground app given back at once, CPU before/after measured 20 s after each step and written to Activity and the in-effect list |

Suggested order: foundation gaps; WP-01 to WP-05; WP-06 to WP-08 and WP-26; the in-progress items as their agents report;
WP-09 to WP-17; WP-23 to WP-25, WP-27 to WP-30.

## Still open in v1 (2026-10-23)

Built this round: WP-02, WP-03, WP-04, WP-05, WP-06, WP-07, WP-08 (Why slow and Reports pages), WP-26, WP-28, WP-29, WP-37 (Linux traffic via
`sock_diag`; Windows says it needs administrator rights). **Not built and still in v1**, so the release waits for them:

- WP-01 is built; what is left of it is verification on real machines (testing-manual H1-H15: install, prompt, kill, remove on each OS), a code-signature check of the caller once there is a Developer ID / Authenticode certificate, SMAppService registration on macOS, on-demand start (the helper stays resident and idle), a Windows installer hook that offers to remove the helper, and a Linux entry for all-users startup (system units).
- WP-09 right-click search, WP-10 watched folders, WP-12 counters browser, WP-13 extras,
  WP-14 backup checks, WP-15 pack data beyond the cleanup packs, WP-16 web API, WP-17 Explain via Pitwall, WP-23 to WP-25,
  WP-27 CLI, WP-30 suite tie-ins, WP-31 session recording, WP-33 to WP-35.
- Parts of built packages: scheduled cleanups, a disk-map screen, Docker image removal, the weekly OS notification (needs an
  agent-to-shell push), score recording in the sampler (the trend only fills while the app asks for health), engine-stored
  diagnosis history, TRIM and network tuning items, Windows IPv6 traffic.
- Not verified on a real desktop: clicking the OS notification and the native save dialog from a debug run (needs a person or
  computer-use session); the Windows and Linux adapters for tuning, cleanup and traffic run only in CI.

CI (2026-10-23): run 7 (56b77e8) had format, clippy, macOS tests, frontend, both Playwright projects and all three bundles green;
rust tests failed on Ubuntu and Windows and were fixed in 72196b3. Run 8 (72196b3) had every test job green; its Ubuntu bundle was
cancelled at the coordinator's request and the Windows and macOS bundles passed. The last commit (the stale-installer guards in
the bundle job) has not run in CI yet: dispatch once after Debrief ships.

Nothing here has been moved to Later; any item moved out of v1 is a user decision.

## Known gaps in the foundation

What the code does today, read from the code on 2026-10-22. Each is a small task for whoever owns the file.

| # | Gap | Where | Fix |
| --- | --- | --- | --- |
| G1 | Done in WP-04 (`Os::Linux`, XDG roots, Linux categories). Was: the cleanup category `Os` enum has `Windows`, `Macos`, `Any` but no `Linux`, so Linux has no categories or roots | `crates/dyno-core/src/cleanup/catalog.rs`, `crates/dyno-agent/src/cleanup/roots.rs` | Add `Os::Linux`, XDG roots (`~/.cache`, `~/.local/share/Trash`, `~/.cargo`, …), catalogue entries and tests |
| G2 | `StartupLocation` has Windows and macOS variants only; the Linux startup adapter (XDG autostart, systemd user units) has nowhere to put its entries | `crates/dyno-core/src/startup.rs`, `crates/dyno-platform/src/linux/startup.rs` | Add `XdgAutostart`, `SystemdUserUnit` (and `needs_admin` for system units), map them, keep the wire contract in step |
| G3 | On Linux the shared Unix `SetPriority` calls `setpriority` on the main thread only (Linux priorities are per thread); efficiency mode and affinity already walk the threads | `crates/dyno-platform/src/unix_control.rs`, `linux/os_control.rs` | Walk the process's threads for `SetPriority` on Linux (best effort for threads that exit); test with a multi-thread dummy |
| G4 | The history `crashes` list is always empty and the health report gets `crashes_7d: 0`, so the crashes chart and the "crashing apps" health reason have no data | `crates/dyno-agent/src/handlers.rs` (`history`, `health`) | Done in WP-06: `history.crashes` is per local day from stored AppCrash issues, `crashes_7d` and the weekly report count them |
| G5 | Plugins are wired minimally: the agent starts the host on first `Request::Plugins` and lists plugins; sensor, cleanup-proposal and rule-action contributions are not connected to the sampler, the scanner or the engine | `crates/dyno-agent/src/handlers.rs`, `sampler.rs`, `crates/dyno-plugin-host` | Connect contributions through guarded adapters (ADR 0007), trim the host to sensors, cleanup proposals and rule actions |
| G6 | Done in WP-01 (ADR 0016). Was: the helper was not built; `crates/dyno-helper/src/main.rs` was `fn main() {}` | `crates/dyno-helper` | done |
| G7 | Cleanup, `DiskUsage`, tuning and the weekly report are built (WP-04, WP-05, WP-08); no request answers `Unsupported` for being unbuilt any more | `crates/dyno-agent/src/handlers.rs` | done |
| G8 | Windows wires sensors (PDH), process control and user activity only. Process details use `NoDetails`; file locks (Restart Manager), startup entries and facts are the in-memory fakes | `crates/dyno-platform/src/windows/mod.rs` | Windows details probe (command line, modules, connections, signature), Restart Manager locks, registry / Startup folder / scheduled-task startup, facts (disk health, updates, power plan); verify on the Windows CI runner |
| G9 | The doc comments on `Overhead::agent_cpu` and `Load::own_cpu` say "percent of the machine", but the sampler multiplies by the core count: the value is a share of one core (ADR 0002 section 4) | `crates/dyno-core/src/protocol.rs`, `sampling.rs` | Fix the comments (and the UI label) or change the unit; keep the budget comparison consistent |
| G10 | The CPU taming, Cleanup and Health screens may still be placeholders in the UI (Why slow and Reports are built) although the engine answers `WhySlow`, `Rules`, `SaveRules`, `SetProfile`, `PauseRules`, `Health`, `StartupEntries`, `SetStartupState`, `Alerts`, `Anomalies` | `app/src/app/features/*`, `app/src/app/core/api.types.ts`, the mock | WP-02, WP-03, WP-08 |
| G11 | Done in WP-14 (`dyno_platform::toolpath`, ADR 0019). Was: the login-shell `PATH` resolver did not exist; nothing started user tools | `crates/dyno-platform/src/toolpath.rs` | done: bounded, timed out, cached, with a validated per-tool override; WP-15 reuses it |
| G12 | On Windows nothing restarts a crashed agent until the app or the next logon does | `crates/dyno-agent/src/service` | Optional: a Task Scheduler "restart on failure" entry |
| G13 | Dyno's overhead was measured once, under a high load average, on one machine; there is no CI artifact (`dyno-agent bench`) per OS | `docs/architecture.md`, `.gitea/workflows/build.yml` | Run `bench` on release builds on a quiet runner per OS, record load next to the numbers, fail on a regression |
| G14 | ~~The tuning plan input to the health report is empty~~ fed from the tuning catalogue (done with WP-05) | `crates/dyno-agent/src/handlers.rs` | WP-05 |
| G15 | CI could not be run when it was written: the workflow, `scripts/check-bundle.sh`, `scripts/check-capabilities.mjs` and `scripts/ci-prune.sh` are validated as YAML and with shell syntax checks and unit tests only | `.gitea/workflows/build.yml`, `scripts/` | Run on the three runners and fix what the first runs show |

## Dyno's own overhead

Measured with `dyno-agent bench 25 none|summary|processes` on the 14-core M4 Mac mini, **while the load average was about
55 to 85**: about 2.0 % (idle, nobody watching), 2.7 % (a summary subscriber) and 3.2 % (a processes subscriber) of **one**
core, and about 24 MB resident (0.15 to 0.25 % of the whole machine). Because the machine was busy, treat these as upper
bounds. The target is at most about 1 % of the machine. See [architecture.md](architecture.md#dynos-own-overhead) and G13.

## WP-01 Privileged helper

- **Scope.** The `dyno-helper` executable and the agent-side installer from ADR 0006: a closed set of `HelperAction`s
  (`SetPowerScheme`, `SetVisualEffects`, `SetBackgroundAppsDisabled`, `SetIndexingExcluded`, `SetDefenderExcluded`,
  `SetLowPowerMode`, `SetMachineStartupEnabled`), each validated in the core and again in the helper, previewed, approved with the
  OS prompt (UAC `runas`, macOS authorisation / `SMAppService` daemon approval, Linux `pkexec`), recorded with its inverse; an
  uninstall action; the helper exits when idle; private endpoint with token and peer-pid check over `dyno-ipc`.
- **Acceptance.** Declining the prompt leaves Dyno fully working; a forged or out-of-set frame is refused and logged; every
  action has a test with a fake backend that applies and reverts it; path arguments are canonicalised and system roots
  refused; the first-admin-action flow matches the sequence diagram; nothing runs elevated except the helper.
- **Depends on.** Nothing; WP-03, WP-05 and parts of WP-04 consume it.
- **Touches.** `crates/dyno-helper/**`, `crates/dyno-core/src/helper.rs`, `crates/dyno-agent/src/handlers.rs` (a helper client
  module, new file), `crates/dyno-ipc` (peer checks), installer config (`app/src-tauri/windows`, `macos`), docs/adr (a new ADR
  for the install flow if it deviates).
- **Built (2026-10-23).** [ADR 0016](adr/0016-privileged-helper-install-and-ipc.md) settles the install flow (one elevated
  `dyno-helper install` command, a resident idle system service, one authorised user and program, token, audit log). Code:
  `crates/dyno-helper` (daemon, `dispatch`, `auth`, `audit`, `install` plans, `winsvc`), `dyno_core::{helper, helper_wire,
  helper_paths}`, `dyno_ipc` (fixed endpoints, peer uid), `dyno_platform::{elevate, tuning_admin}`, `dyno_agent::{helper_link,
  helper_centre}`, the Settings > Helper card and the install dialog in the UI. Verified: unit and in-process IPC tests with a
  fake system everywhere, the agent-to-daemon path end to end with its real authentication, Windows and Linux type-check and
  clippy from the Mac, the Windows-only code on the Windows CI runner. Not verified: a real install on any OS (manual list H1-H15).

## WP-02 CPU taming screen and profiles

- **Scope.** The CPU taming UI: rules list with enable switches (all shipped disabled), the escalation ladder editor, profiles
  (Normal, Gaming, Meeting, On battery, Building code) with a switcher in the shell (`profile-switcher.ts` exists), the
  protected list with the reason a process is protected, a pause switch, the live "actions in effect" list with release, and
  the Activity list with undo. Starter rules and profile membership as data in the core.
- **Acceptance.** Nothing acts until a rule is enabled; enabling shows what it will do; a rule never touches the foreground
  app or a protected process; every step shows in Activity with an undo; screens match the mock and the real agent; Playwright
  flows cover enable, release and undo.
- **Depends on.** G10; G3 for Linux parity.
- **Touches.** `app/src/app/features/rules/**`, `state/rules.store.ts`, `core/api.types.ts`, `core/mock/**`,
  `crates/dyno-core/src/rules/model.rs` (starter rules), docs/screenshots (`rules-*.png`).

## WP-03 Health score and startup manager screen

- **Scope.** The Health page (tuning moved to its own page, WP-05): the health score with its reasons and fixes (previews before anything changes), the
  startup manager (list with impact, enable or disable per entry through `StartupItems`, undo; machine-wide entries through the
  helper), and the dashboard hero already built.
- **Acceptance.** Disabling a user startup entry is undoable; machine-wide entries show "needs administrator rights" until the
  helper exists; impact is measured, not guessed; the page works with no data (a fresh install).
- **Depends on.** G2 (Linux), G8 (Windows), WP-01 for machine-wide entries.
- **Touches.** `app/src/app/features/health/**`, `state/health.store.ts`, `crates/dyno-core/src/health.rs`, `startup.rs`,
  `crates/dyno-platform/src/{windows,macos,linux}/startup.rs`, `docs/screenshots/health-*.png`.

## WP-04 Cleanup with quarantine and undo

- **Scope.** Scanning, previewing and deleting per ADR 0005: the catalogue (system temp, caches, crash dumps, logs, browsers,
  developer build output and package caches, Downloads, Docker, Xcode and Android SDK), the scanner and deleter in the agent
  (re-check before each delete, never links, never personal folders unless the category's root), Recycle Bin / Trash and a 7-day
  quarantine with restore, the disk-space map (`DiskUsage`), scheduled cleanups of the categories the user enabled, in-use files
  reported with the program holding them.
- **Acceptance.** A scan deletes nothing; a run deletes only items of a scan younger than an hour that the user selected; a
  symlink, a `..` path and a path outside the root are refused (tests for each); a locked file is skipped with the locker's name;
  undo restores from quarantine; the preview lists every item and its size; Linux has categories (G1).
- **Depends on.** G1; WP-15 for pack-defined categories.
- **Touches.** `crates/dyno-core/src/cleanup/**`, `crates/dyno-agent/src/cleanup/**` (scanner, deleter, quarantine), handlers,
  `app/src/app/features/cleanup/**`, mock, `docs/screenshots/cleanup-*.png`.

## WP-05 Tuning catalogue

- **Scope.** A catalogue of reversible suggestions that change something only on a click: power plan or mode, sleep and
  hibernation timers, startup trimming, storage (TRIM, indexing exclusions for developer folders), developer tweaks (antivirus
  exclusions for build folders, with an explicit warning), visual effects, network, battery charge limit where the OS exposes it.
  Each is `check` (current value), `apply` and `undo` data, run through the helper when it needs rights. **Excluded by decision:**
  registry cleaners, RAM boosters, debloat scripts.
- **Acceptance.** Every suggestion previews its exact change and its inverse; applying records an `UndoRecord::Tuning`; a change
  made outside Dyno since is detected before undo; nothing in the catalogue runs a string as a command.
- **Depends on.** WP-01; WP-15 for pack data.
- **Touches.** `crates/dyno-core/src/tuning.rs`, new `tuning/catalog.rs`, handlers (`Tuning`, `ApplyTuning`, `RevertTuning`),
  `app/src/app/features/health/**`, docs.

## WP-06 Issues with statistics

- **Scope.** A curated list of OS problem events collected by the agent: application crashes and hangs, bad shutdowns, hardware
  warnings (disk, memory, thermal), failing services, memory exhaustion, update and driver failures. Per OS sources (Windows
  Event Log and reliability records, macOS diagnostic reports and the unified log, `journalctl`/`coredumpctl` on Linux), normalised
  into one `Issue` type, stored in SQLite (ADR 0008 reserves the table), with a **daily stability score**, per-app and per-kind
  **leaderboards**, trends, and **Open in Debrief** (export the window to Debrief). Fills the crashes chart and health reason (G4).
- **Acceptance.** Sources are read with timeouts and without file contents in logs; an unreadable source is an
  "unavailable: reason" entry, never an error for the page; the score is a pure function with tests; an issue links to the
  evidence (event text is shown, never `[innerHTML]`).
- **Status (built).** `dyno_core::issues` (types, text hygiene, `day_score`, leaderboards, trend, crash loops, JSON-lines export),
  `dyno_platform::issues` (pure parsers for every OS plus readers behind `IssueSource` and a `ToolRunner` seam), the agent's
  `issues` centre (background read every 15 minutes, panic-contained readers, "unavailable: reason" per source), the `issues`
  table, the Issues page. Deviations and gaps: Windows reads the Event Log through `wevtutil` (the OS's own front end to the
  API, with timeouts) instead of native `EvtQuery` FFI, which could not be type-checked on the build Mac; there is no
  reliability-record reader and no overheating source on Windows; "Open in Debrief" is **Export for Debrief** (JSON lines you
  open in Debrief), launching Debrief waits for WP-30; hang reports on macOS come from `.hang` and `.spin` files only; the
  Windows and Linux readers have run only against fixtures (CI runs the parsers on every OS, the real tools only on their own
  runners); `restarts` in the crashes chart stays 0 (no OS log says an app was relaunched). Wording: serious issues raise
  alarms (`issue:<kind>`, one per issue, only when recent); a crash loop (3 crashes of one app in an hour) feeds the
  crash-loop alert and Why slow.
- **Depends on.** G4, WP-15 optional for source packs; Debrief export format.
- **Touches.** new `crates/dyno-core/src/issues.rs`, `crates/dyno-platform/src/{windows,macos,linux}/issues.rs`, `store.rs` (migration),
  handlers, `app/src/app/features/issues/**` (new page), nav, mock, screenshots.

## WP-07 New and unusual processes

- **Scope.** A transparency tool, not an antivirus. A baseline of programs seen on this machine (ADR 0008 reserves the table) and
  flags: first seen, unsigned or unknown publisher, running from an unusual location (temp, downloads, user-writable system
  paths), name masquerading as a system process, odd behaviour (sudden network, many children), new persistence (a new startup
  entry or scheduled task). Each flag explains itself and offers "protect", "ignore" or "search the web" (WP-09).
- **Acceptance.** No verdicts ("malware"): wording is "unusual because …"; a pure scoring function with tests for each flag;
  a fresh install learns a baseline silently for a configurable period instead of flagging everything; flags never trigger an
  action by themselves.
- **Status (built).** `dyno_core::unusual` (pure `assess` with a test per flag; `Baseline::observe` with silent learning,
  default 7 days, 0 to 60 in Settings, the very first look always silent), agent `unusual` centre (look every 30 s, signatures
  of programs never seen checked 3 at a time off the sampler, one activity line when a program becomes unusual), tables
  `baseline` and `baseline_startup`, "New and unusual" view on the Issues page. Actions: Protect (adds to the protected list),
  Hide, Restore; none acts on the program. Gaps: "search the web" waits for WP-09; "sudden network" is approximated as many
  open connections (a rate needs per-process traffic, WP-37 has it on macOS and Linux only); signatures need process details,
  so Windows flags no "unsigned" until G8.
- **Depends on.** G8 (Windows details and signatures), WP-06 for the shared page.
- **Touches.** new `crates/dyno-core/src/unusual.rs`, `store.rs`, handlers, `app/src/app/features/issues/**` or `processes/**`.

## WP-08 Alerts and the weekly report

- **Scope.** The Reports page: alert history (low disk, overheating, crash loops, rule actions), notification delivery (OS
  notifications, quiet hours), and the weekly report (`WeeklyReport`: health trend, top consumers, disk forecast, issues, what
  changed). Exportable.
- **Acceptance.** Alerts are rate limited and deduplicated; the report is a pure function over stored data with tests; "nothing
  to report" is a valid report.
- **Status.** Report, page and export are built. Open: the alert history on the Reports page, the optional weekly OS notification
  (needs the agent to push a notice to the shell; design before building), and the score is recorded only while the app asks
  for the health report (move recording into the sampler).
- **Depends on.** WP-06 for issues; G10.
- **Touches.** `crates/dyno-core/src/{alerts,report}.rs`, handlers, `app/src/app/features/reports/**`, `state/alerts.store.ts`.

## WP-09 Right-click search

- **Scope.** Right-click a process, file or event and "Search the web": builds a **redacted** query (user name, host name, home
  paths, serials removed), shows it **editable**, and sends nothing until the user clicks Search; opens the default browser.
- **Acceptance.** Redaction is a pure core function with tests (paths, user names, GUIDs, IPs); nothing is sent without the click;
  the editable text is what is used.
- **Depends on.** Native-feel context menu (ADR 0009).
- **Touches.** new `crates/dyno-core/src/redact.rs`, `app/src/app/shared/copy-menu.ts` (menu entries), shell command to open the
  browser (declared in `app/src-tauri/src/commands.rs` by the integrating agent).

## WP-10 Watched folders with keep-for times

- **Scope.** The user watches folders and sets a keep-for time (from 10 minutes to 10 days); file-change notifications (FSEvents,
  inotify, `ReadDirectoryChangesW`) plus a periodic sweep remove expired files to the Trash or quarantine, with a dry run,
  templates (Downloads installers, screenshots, build output) and an activity entry with undo per sweep.
- **Acceptance.** Admission through the same policy as cleanup (ADR 0005): never links, never outside the watched root, never a
  personal folder unless the user chose it; a dry run shows exactly what would go; notifications are debounced and a missed
  event is caught by the sweep.
- **Depends on.** WP-04 (policy, quarantine, Trash).
- **Touches.** new `crates/dyno-core/src/watch.rs`, `crates/dyno-agent/src/watch/**`, settings (versioned migration), UI page.

## WP-11 Benchmarks on click

- **Status.** Built (ADR 0015). `crates/dyno-bench` (kernels and the child process), `dyno_core::bench` (types, plan validation,
  comparison, bounded history), `dyno-agent/src/bench.rs` (one run at a time, per-kind timeout, scratch folder removed by the
  agent), requests `benchmarks`, `startBenchmark`, `cancelBenchmark`, `deleteBenchmark`, `compareBenchmarks`; UI page
  `/benchmarks`; screenshots `benchmarks-*.png`.
- **Open.** GPU compute (reported unavailable), a benchmark offered from the tuning confirmation dialog, cancelling a kind
  that is already running (cancel acts between kinds; the timeout bounds each).

## WP-37 Network: open ports and connections per process

- **Status.** Built, observe-only (ADR 0015). `dyno_core::network` (view, filters, exposure, tracker), `NetworkSockets` port
  implemented with `netstat2` in `dyno-platform/src/network.rs`, `dyno-agent/src/network.rs` (cached read, background look
  every 30 s, activity entries for new exposed listeners and new remote hosts), request `network`; UI page `/network`.
  **Look up IP** (`dyno_core::ip_lookup`, request `lookupIp`, Settings ▸ Network ▸ IP lookup) opens the user's https
  template in the browser; private, loopback, link-local and CGNAT addresses are refused. No country database is bundled.
- **Page.** One dense table with a sticky header and virtualised rows, group rows by program or remote host (collapsed state
  remembered), a summary strip, a detail panel and keyboard control; program icons are initial avatars.
- **Open.** Reverse DNS through the OS resolver (on demand, switchable) and the remote host name column; per-connection traffic
  rates, Windows traffic through the helper (IPv6 and ETW as well), UDP bytes; per-process traffic rates and sparklines in group rows; a row context menu; the user column in the detail panel;
  the tab in process details; resolving other users' sockets through the helper; rules that alert on a new non-loopback
  listener; redacting remote IPs in diagnostics bundles unless the user opts in; ASN column (the ASN lookup template is
  stored but no ASN is known yet).

## WP-12 Performance counters browser

- **Scope.** Browse and chart raw counters: Windows PDH (`PdhEnumObjects`), macOS and Linux equivalents (`sysctl`, `/proc`,
  `/sys`, `iostat`-like sources), search, pin to the dashboard.
- **Acceptance.** Counters are read through a port with timeouts; a counter that disappears is shown unavailable with the reason;
  a pinned counter survives restart.
- **Depends on.** G8 for Windows.
- **Touches.** `crates/dyno-core/src/ports.rs` (a counters port, integrating agent), `crates/dyno-platform/**`, UI page.

## WP-13 Extras

Each is small and independent; all are core logic plus an adapter plus a card or page.

| Extra | Scope | Notes |
| --- | --- | --- |
| What changed | A timeline of installs, updates, new startup items and setting changes around a time | Reuses snapshots and the timeline events table |
| Why won't it sleep / what woke it | `pmset -g assertions` and wake reasons (macOS), `powercfg /requests` and `/lastwake` (Windows), `systemd-inhibit --list` (Linux) | Read only |
| Boot analysis | Boot duration and what slowed it (`boots` table exists) | Per OS sources |
| Network and energy per app | Per-app bytes (`nettop`, ETW or counters, `/proc`) and energy impact where the OS exposes it | Adds fields to app minutes |
| Inventory report | Hardware and software inventory export | Local file only |
| GPU per process | Per-process GPU use where the OS tells | Linux needs vendor interfaces |
| Large and duplicate file finder | Scan a chosen folder for big or duplicate files, never delete without the cleanup policy | Reuses WP-04 admission |
| Maintenance windows | Times when Dyno's heavier work (scans, cleanups) may run | Setting plus scheduler |

## WP-14 Backup health checks

- **Scope.** Check that backups ran and succeeded: Time Machine (`tmutil`), Windows Backup and File History, restic, Borg and
  Duplicati (via their logs or CLIs, declared as tier-2 detector packs). Dyno never runs or manages backups.
- **Acceptance.** Tools are run with timeouts and the user's login-shell `PATH` (G11); "no backup tool found" is a normal state;
  a stale backup raises an alert with a plain sentence.
- **Depends on.** G11, WP-15, WP-08.
- **Touches.** `crates/dyno-core/src/backup.rs` (new), pack detectors, adapter command runners, UI card.

## WP-15 Declarative TOML packs

- **Scope.** Tier 2 of ADR 0007: cleanup packs (developer caches, browsers, Docker, Xcode and Android SDK, game launchers), tuning
  packs (check, apply, undo data), process rule packs (taming rules, known helpers) and backup detectors. A pack is parsed once
  at the edge (`deny_unknown_fields`, size and count limits, path templates expanded by the core), validated, and every path still
  passes the cleanup policy; invalid packs are skipped with a message naming the file and line; a format version gates loading.
  Packs ship in the app and may be dropped into the user's `packs/` folder.
- **Acceptance.** A pack cannot widen what Dyno may delete or change (test with a hostile pack); a corrupt pack never stops the
  app; the default profile file documents every key (the schema is its own documentation, like Debrief's).
- **Depends on.** WP-04.
- **Touches.** new `crates/dyno-core/src/packs/**`, `crates/dyno-agent` (loader), a `packs/` data folder, docs/plugins.md.

## WP-16 Optional local web API

- **Scope.** Off by default (`Settings::web_api`): a loopback-only HTTP API for read-only status and metrics, protected by a key
  kept in the OS credential store; never exposed beyond loopback unless the user opts in explicitly.
- **Acceptance.** Binds `127.0.0.1` only by default; a request without the key gets 401; rate limited; no action endpoints in v1;
  documented.
- **Touches.** new `crates/dyno-agent/src/web/**`, settings, docs.

## WP-17 Explain this via Pitwall

- **Scope.** "Explain this" on a diagnosis, an issue or an unusual process sends a **redacted** summary (WP-09's redaction) to the
  user's local Pitwall server, only on click, and shows the answer inline.
- **Acceptance.** Local only; nothing is sent without the click; the request text is shown first; a missing Pitwall is a clear
  message.
- **Depends on.** WP-09, WP-06, WP-07; Pitwall's API.
- **Touches.** shell command, `app/src/app/core/backend.ts`, UI.

## WP-19 Per-core CPU view

- **Scope.** A per-core view of CPU: a dashboard tile toggle (total or per core), performance and efficiency cores grouped and
  labelled, per-core frequency, a CPU detail page with a heatmap over time, per-core min/avg/max rollups in history, and
  per-core bars in the tray item.
- **State.** Core types, platform probes, the agent's sampling and the per-core rollups are committed (`c2b757f`); the UI parts
  are in progress.
- **Acceptance.** A machine without per-core data says why; P/E grouping is correct on Apple Silicon and Intel hybrid parts and
  absent elsewhere; the heatmap prints the numbers as well as colours (ADR 0011).
- **Touches.** `dyno-core` metrics and history, `dyno-platform/**`, `app/src/app/features/dashboard/**` and a CPU detail page,
  `dyno_core::tray`.

## WP-20 Memory modules inventory

- **Scope.** The installed memory modules (size, type, speed, slot, manufacturer) drawn as slots: Windows through WMI, macOS
  through `system_profiler` (including Apple Silicon's unified memory, drawn as one package), Linux through DMI tables, which
  need the helper and are read only on a click.
- **Acceptance.** Empty slots are drawn empty; an unreadable source says why; the Linux read asks for the helper first
  (ADR 0006) and the app works without it.
- **Depends on.** WP-01 for Linux. **Touches.** `dyno-platform/**`, a facts type in the core, a memory detail page.

## WP-21 Customisable widgets and tray items

- **Scope.** ADR 0012: dashboard edit mode, a gallery of widgets, presets, and several menu-bar or tray items (iStat and Stats
  style) with per-OS rendering notes.
- **Acceptance.** Layouts are versioned settings that survive damage; unknown kinds are dropped with a note; the tray item count
  is limited to what the OS supports; presets are undoable.
- **Touches.** `dyno_core::{settings,tray}`, `app/src/app/features/dashboard/**`, `app/src-tauri/src/tray.rs`.

## WP-22 Native tray popover

- **Scope.** A rounded, material-backed popover opened from the tray (macOS vibrancy, Windows Mica where available, a plain
  themed window on Linux), showing the compact dashboard.
- **Acceptance.** Looks native on each OS in both themes; closes on blur; honours ADR 0009.
- **Touches.** `app/src-tauri/src/{tray,window,placement}.rs`, `app/src/app/features/tray/**`.

## WP-23 Throttling detector

- **Scope.** Detect that the CPU or GPU is running slower than it could under load, name the cause (thermal limit, power limit,
  battery saver) and quantify the impact (percent of expected frequency lost, for how long).
- **Acceptance.** A pure detector over frequency, temperature and power readings with tests per cause; an unavailable input
  gives "cannot tell", never a guess; the result feeds Why slow and alarms.
- **Touches.** new `dyno-core/src/throttle.rs`, adapters for frequency and limit flags, `diagnosis.rs`.

## WP-24 Disk and battery life forecasts

- **Scope.** SSD wear (SMART or NVMe data, total bytes written against rated TBW) as a remaining-years estimate; battery
  capacity trend and cycle count as "replace in about N months".
- **Acceptance.** Estimates show a range and the data they stand on; too little history says so; no estimate without data.
- **Touches.** `dyno-core/src/forecast.rs`, facts adapters, history of capacity and wear.

## WP-25 "Is this normal?" baselines

- **Scope.** Learn per metric and app a baseline by time of day and day of week from history; highlight values outside it on
  the dashboard and feed alarms.
- **Acceptance.** A pure statistics function with tests; a new install learns silently for a configurable period; highlights
  carry the words ("higher than usual for Tuesday mornings"), not colour alone.
- **Touches.** `dyno-core/src/stats.rs` and `anomaly.rs`, history queries, dashboard.

## WP-26 Alarms view and custom alerts

- **Scope.** ADR 0013 in full: the pure state machine, custom alerts on any metric using the taming rule engine in
  notification-only mode, Active, History and Statistics views, acknowledge, shelve, mute, notes, Open in Debrief, tray and
  dashboard badge, native notifications with Acknowledge and Snooze, quiet hours with a critical bypass, bounded SQLite
  retention.
- **Acceptance.** Every (state, event) pair is tested; repeats de-duplicate; shelving expires correctly across a reboot; active
  alarms are never pruned; notifications never contain file contents.
- **Touches.** `dyno-core/src/alarms.rs` (in progress), `alerts.rs` (superseded), `store.rs`, handlers, `features/alarms/**`.

## WP-27 `dyno` CLI

- **Scope.** ADR 0014: `dyno status --json`, `why-slow`, `top`, `alarms`, over the same protocol and discovery file; a design note
  already sits in `crates/dyno-agent/src/server.rs`.
- **Acceptance.** Refuses a wrong protocol version with a clear message; read-only unless an explicit acting flag is given;
  output is stable JSON.
- **Touches.** new crate `crates/dyno-cli`, workspace `Cargo.toml` (integrating agent), docs.

## WP-28 Flight recorder

- **Scope.** The last 10 minutes at 1 s kept in memory, bounded; saved automatically when an alarm fires or on "Capture"; a
  timeline of captured snapshots; overhead within about 1 % of the machine.
- **Acceptance.** Memory is capped by construction (ring buffer); saving writes nothing outside the data folder; the overhead
  is measured with `dyno-agent bench` and recorded.
- **Touches.** new `dyno-core/src/flight.rs`, the sampler, `store.rs`, UI.

## WP-29 Diagnostics bundle

- **Scope.** One click builds a zip: inventory, health score and reasons, recent issues and alarms, a flight snapshot and the
  weekly summary, with user names, paths and the machine name redacted (Debrief style), previewed before it is saved.
- **Acceptance.** Redaction is a tested pure function; the preview shows exactly the files and text that go in; nothing is sent
  anywhere.
- **Depends on.** WP-06, WP-26, WP-28, WP-13 (inventory). **Touches.** `dyno-core` redaction (shared with WP-09), agent export,
  UI dialog.

## WP-31 and WP-33 to WP-36 (from competitive research)

- **WP-31 Session recording.** Record chosen metrics to CSV or Parquet (start/stop, bounded size), with a built-in viewer
  (chart + table, open an old file). Touches: agent recorder/export, UI viewer page. Acceptance: a 10-minute session round
  trips and charts; file size is bounded; Parquet opens in Debrief.
- **WP-33 VirusTotal hash search.** Context menu on a process or file: compute the SHA-256 locally (once, cached), open the
  browser at the VirusTotal file page for that hash. Nothing is uploaded and the hash is shown before opening.
- **WP-34 Startup filter.** Hide OS-owned and signed entries in the startup manager (a toggle, remembered), counts shown.
- **WP-35 Network change events.** Wi-Fi joins/leaves, VPN up/down and IP changes recorded as timeline events (per-OS
  adapters in dyno-platform; no polling of the whole network stack: use OS change notifications or a slow check).
- **WP-36 Taming acceptance rules.** A rule can be temporary (expires), foreground-aware (never touches the app in front),
  self-restoring (releases when the condition ends or Dyno exits) and proves its effect (CPU before/after measured and
  shown in the activity entry). Builds on the rules engine (ADR 0004).

**Sampling lesson (applied):** polling the macOS volume-space API often can stall Finder (an iStat Menus experience). The
common sensors now refresh free space at most once a minute; any new feature that touches volume space, the network stack
or other shared OS services must poll rarely or use change notifications.

## WP-30 Suite tie-ins

- **Scope.** The installed-apps inventory feeds Spotter's "watch for updates"; issues and alarms offer Open in Debrief;
  Pitcrew shows Dyno's machine health when both run; Pitwall's Explain this (WP-17). All through ADR 0014's protocol, on the
  user's click or opt-in.
- **Acceptance.** Each integration declares what it reads; none writes; each degrades to nothing when the other app is absent.

## Later (after v1)
<a id="after-v1"></a>


Not part of v1; anything new goes here.

- **Multi-PC LAN view.** See several of your own PCs on one dashboard over the LAN, opt-in, read-only, over **TLS with a
  pre-shared key**. Needs its own ADR (discovery, key exchange, certificate handling) before any code; depends on WP-16 and
  WP-27.
- Further ideas land here as they come up.

## Screenshot checklist

The UI agent produces `docs/screenshots/<screen>-<dark|light>.png` (`npm run screenshots` in `app/`, against the mock). Each
feature needs its images; a row is done when both exist and show the built screen, not a placeholder.

| Feature | Screenshot | State |
| --- | --- | --- |
| Dashboard (live tiles, diagnosis, health) | `dashboard-dark.png`, `dashboard-light.png`, `dashboard-full-*.png` | built |
| Processes (grouped tree, details, undo) | `processes-*.png` | built |
| Settings | `settings-*.png` | built |
| Tray dropdown | `tray-*.png` | built |
| Why slow | `why-slow-*.png` | real screen built; capture pending |
| CPU taming | `rules-*.png` | placeholder; real screen with WP-02 |
| Cleanup | `cleanup-*.png` | page built; recapture the screenshots |
| Health and tuning | `health-*.png` | placeholder; real screen with WP-03 and WP-05 |
| Reports and alerts | `reports-*.png` | real screen built; capture pending |
| Issues with statistics | `issues-*.png` | built |
| Unusual processes | `unusual-*.png` | built |
| Watched folders | `watched-folders-*.png` | not started (WP-10) |
| Benchmarks | `benchmarks-*.png`, `benchmarks-running-*.png`, `benchmarks-compare-*.png` | built |
| Network | `network-*.png`, `network-connections-*.png` | built |
| Performance counters | `counters-*.png` | not started (WP-12) |
| Backup health | `backup-*.png` | built |
| Per-core CPU tile (total and per-core toggle, P/E grouping) | `cpu-per-core-*.png` | in progress (WP-19) |
| CPU detail with per-core heatmap | `cpu-detail-*.png` | in progress (WP-19) |
| Tray popover on a desktop backdrop | `tray-popover-*.png` | in progress (WP-22) |
| Widget gallery | `widget-gallery-*.png` | in progress (WP-21) |
| Dashboard edit mode | `dashboard-edit-*.png` | in progress (WP-21) |
| Presets | `presets-*.png` | in progress (WP-21) |
| Customised menu bar (several tray items) | `menu-bar-*.png` | in progress (WP-21) |
| Memory slot drawings, including Apple Silicon unified memory | `memory-slots-*.png`, `memory-slots-apple-silicon-*.png` | in progress (WP-20) |
| Alarms: Active | `alarms-active-*.png` | planned (WP-26) |
| Alarms: History | `alarms-history-*.png` | planned (WP-26) |
| Alarms: Statistics | `alarms-statistics-*.png` | planned (WP-26) |
| Diagnostics bundle preview | `diagnostics-bundle-*.png` | planned (WP-29) |
| Throttling and forecasts | `throttling-*.png`, `forecasts-*.png` | planned (WP-23, WP-24) |
| Extras: What changed, Sleep, Boot, Apps, GPU, Files, Inventory, Maintenance | `extras-<tab>-*.png` | built (WP-13) |
| Temporary rule with measured proof | `rules-temporary-*.png` | built (WP-36) |
