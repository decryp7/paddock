# Dyno roadmap

What is built is in [features.md](features.md); this page is everything that is not. **Dyno 1.0.0 is a monitor**
([ADR 0021](adr/0021-monitoring-only.md)): it watches and never changes the machine. Work packages (WP) are written so an
agent or contributor can pick one up: scope, acceptance criteria, dependencies and the files it touches. Shared integration
points (`lib.rs`, `protocol.rs`, `ports.rs`, the workspace `Cargo.toml`, `api.types.ts`) are changed by the integrating
agent only (AGENTS.md "Working in parallel"). Every package ships tests, and every robustness behaviour has a test that
triggers the failure.

Decisions already taken: three-tier extensibility ([ADR 0007](adr/0007-extensibility-three-tiers.md), read-only since
0021), SQLite ([0008](adr/0008-storage-sqlite-and-retention.md)), a read-only privileged helper, planned for 1.1 and not in 1.0
([0006](adr/0006-privileged-helper-and-observe-only.md), [0016](adr/0016-privileged-helper-install-and-ipc.md),
[0021](adr/0021-monitoring-only.md), ADR 0022), Linux a first-class platform next to Windows and macOS.

## Scope: v1 is "monitor"; the rest is dropped or Later

**v1 (1.0.0) is the monitor**: every work package in the table below that is not marked planned or dropped. There is no cut-down
release inside it. WP-15, 16, 17, 30 and 31 are **planned as Later, after 1.0**. The features that change the machine are
**dropped, not later** ([below](#dropped)).

Status words are honest: **built** (code and tests committed), **in progress** (another agent is working on it), **planned**
(nothing in the code), **dropped** (decided against, ADR 0021).

| WP | Feature | Status |
| --- | --- | --- |
| WP-01 | Privileged helper (read-only) | planned for 1.1, not in 1.0 (ADR 0022); 1.0 runs with the user's rights, marks unreadable figures *Unavailable* with a reason, shows an "Other users" row and has no way to run elevated. The code from before the decision is removed from 1.0 by separate changes |
| WP-02 | CPU taming screen and profiles | dropped ([ADR 0021](adr/0021-monitoring-only.md)) |
| WP-03 | Health score and startup items (view only) | built: Health page with reasons and the startup list; fix buttons and startup changes are dropped (ADR 0021) |
| WP-05 | Tuning catalogue | dropped ([ADR 0021](adr/0021-monitoring-only.md)) |
| WP-04 | Cleanup with quarantine and undo | dropped ([ADR 0021](adr/0021-monitoring-only.md)); the disk map (`DiskUsage`) stays: engine and screen built (the Disk map page) |
| WP-06 | Issues with statistics | built: macOS crash reports and unified log, Windows Event Log (`wevtutil`), Linux journal and `coredumpctl`; stored in SQLite; stability score, leaderboards, trend, JSON-lines export for Debrief; crashes chart, health reason and weekly report filled (G4); crash-loop alert and serious-issue alarms. Linux and Windows readers are verified by fixtures only (see Status) |
| WP-07 | New and unusual processes | built: baseline with silent learning, scoring (first seen, location, system-name lookalikes, signature, many children or connections, new startup entry), "New and unusual" view with hide and restore (protect is dropped, ADR 0021). Windows signatures wait for G8 |
| WP-08 | Alerts and the weekly report | built: weekly report from stored data (health trend, apps, disk forecast, issues, changes), Reports page with week selector, Markdown/HTML export; Why slow page (G10). Planned: alert history on the page, weekly OS notification (superseded in part by WP-26) |
| WP-09 | Right-click search | built: redacted editable text (`dyno_core::weblinks`), the dialog from the Processes, Network, Issues and New and unusual menus, nothing sent until Search |
| WP-10 | Watched folders with keep-for times | dropped ([ADR 0021](adr/0021-monitoring-only.md)) |
| WP-11 | Benchmarks on click | built: CPU, memory and disk (GPU reported unavailable), out of process with a timeout, load stored, before/after comparison, results page (ADR 0015) |
| WP-12 | Performance counters browser | built: Counters page (objects, search, instances), chart any counter with the app's events marked, pinned counters and saved sets in settings, "Add to the dashboard" as a counter widget, `dyno counters`; Windows PDH (`PdhEnumObjectsW`, `PdhEnumObjectItemsW`, one persistent query; type-checked and linted from macOS, run by the Windows runner), macOS `vm_stat`/`netstat -inb`/`iostat -Id`/`sysctl`, Linux `/proc`; IOKit per-device statistics are not read |
| WP-37 | Network: open ports and connections per process, Look up IP | built: listeners, connections, filters, grouping, new-listener/host activity entries, browser lookup (ADR 0015); reverse DNS on request, per-program traffic (macOS `nettop`; Linux `sock_diag` byte counters + `/proc/<pid>/fd`, no admin; Windows per-connection TCP statistics, which need administrator rights, so it says so), row context menu, process-details Network tab, alarms for new exposed listeners and new remote hosts |
| WP-13 | Extras (what changed, sleep, boot, network and energy, inventory, GPU per process, file finder, maintenance windows) | built (feat/wp13-14-36): the Extras page with eight tabs over pure parsers with fixtures ([ADR 0020](adr/0020-extras-backup-checks-and-taming-proof.md)). Verified by fixtures only: the Windows and Linux readers. Not built: per-app network and energy are not stored as app-minute columns (shown live, macOS only for energy), and Linux GPU per process needs NVIDIA's tool |
| WP-14 | Backup health checks | built (feat/wp13-14-36): Time Machine, Windows Backup, File History, restic and Borg judged against a stale limit, alarm per tool, login-shell `PATH` resolver (G11). Duplicati is detected but not readable yet; the detectors are built in until WP-15's packs exist; Windows event IDs are fixture-verified only |
| WP-15 | Declarative TOML packs | planned, Later (after 1.0) |
| WP-16 | Optional local web API | planned, Later (after 1.0) |
| WP-17 | Explain this via Pitwall | planned, Later (after 1.0) |
| WP-19 | Per-core CPU view | built: P/E kinds, per-core frequency and rollups, tile toggle, detail heatmap (tray per-core bars via tray items); Windows kinds CI-only |
| WP-20 | Memory modules inventory with slot drawings | built: parsers (Windows/macOS/Linux DMI), hints, inventory page with slot drawing; the Linux DMI read needs root and says so in 1.0 |
| WP-21 | Customisable widgets (ADR 0012) | built: model, dashboard edit mode/presets, menu-bar items editor; preset save button planned |
| WP-22 | Native tray popover | built: popover.rs + dy-popover; Windows/Linux look CI/manual-only |
| WP-23 | Throttling detector | built: pure detector and episode tracker, read-only readers (macOS `pmset`, Linux sysfs counters; Windows infers), Throttling page, Why slow cause, alarms with evidence; Windows reader CI-only |
| WP-24 | Disk and battery life forecasts | built: robust range forecasts for SSD wear and battery capacity, replacement and clock-change handling, readers (smartctl, ioreg, sysfs, powercfg), Lifespan page, Health, report and alarms; reading SSD wear without smartmontools waits for WP-01 |
| WP-25 | "Is this normal?" learned baselines | built: weekday-and-hour baselines per metric and app with fallbacks, silent learning period, dashboard card in words, optional alarms |
| WP-26 | Alarms view and custom alerts (ADR 0013) | built: Alarms page (active, history, rules), custom alert rules evaluated by the agent, desktop notifications (tauri-plugin-notification), quiet hours |
| WP-27 | `dyno` CLI (ADR 0014) | built and shipped in the installers: `dyno status\|why-slow\|top\|alarms\|snapshot\|counters` (text or `--json`), read-only; refuses another protocol version; the binary is a sidecar of every bundle (docs/cli.md); `end`, `cap` and `undo` are dropped |
| WP-28 | Flight recorder | built: ring, capture, alarm capture and the timeline page |
| WP-29 | Diagnostics bundle | built: redaction (IPv4 and IPv6), preview, zip, a redacted sockets file, the page and the OS save dialog |
| WP-30 | Suite tie-ins (Spotter, Debrief, Pitcrew, Pitwall) | planned, Later (after 1.0) |
| WP-31 | Session recording to CSV/Parquet with a built-in viewer | planned, Later (after 1.0) |
| WP-33 | Right-click "search this file's hash on VirusTotal" (opens the browser only; nothing is uploaded) | built: SHA-256 computed locally and shown first, then the browser opens the VirusTotal page |
| WP-34 | Startup manager filter: hide OS and signed entries | built: engine-side filter with counts, remembered; "signed" needs a publisher, which macOS launch agents do not report yet |
| WP-35 | Network change events (Wi-Fi joins, VPN up/down, IP changes) on the timeline | built: `dyno_core::netchange`, per-OS Wi-Fi readers, slow polling; macOS may hide the Wi-Fi name; Windows and Linux readers are fixture-tested only |
| WP-36 | Taming acceptance rules | dropped ([ADR 0021](adr/0021-monitoring-only.md)) |
| WP-38 | Customizable tray popup widgets | in 1.0, the tag waits for it (user, 2026-10-31); design: family ADR 0035 |

## Still open in v1

- The disk map screen (the engine is built): the clickable ring chart with drill-down, breadcrumb, live scan with Cancel and the table view. The largest, oldest and duplicate-hint tabs and the hidden-space breakdown are for 1.1.
- Dyno 1.0 without the helper (ADR 0022): "Unavailable" with a reason for every unreadable figure, the "Other users" row. No system service is installed and nothing runs elevated.
- Parts of built packages: the weekly OS notification (needs an agent-to-shell push), score recording in the sampler (the trend only
  fills while the app asks for health), engine-stored diagnosis history, Windows IPv6 traffic, and the Windows adapters for
  details, locks, startup and facts (G8).
- Deleting the dropped code and screens: other work packages, tracked outside this page.
- Not verified on a real desktop: clicking the OS notification and the native save dialog (needs a person or computer-use session).

## Dropped

Decided 2026-10-27 and confirmed 2026-10-28 ([ADR 0021](adr/0021-monitoring-only.md)): these will not be built and are not
"later"; bringing one back needs a new ADR.

- The helper's Apply and every write path.
- CPU taming: rules, profiles, pause, the protected list, and the proof of effect (WP-02, WP-36, [ADR 0004](adr/0004-cpu-taming-and-throttling-methods.md)).
- Process actions: end, suspend, resume, priority, efficiency mode, caps, Act, ActOnGroup and Undo.
- Startup-state changes and the health "fix" buttons.
- Cleanup: categories, scan and run, quarantine and restore, scheduled cleanups, Docker image removal, watched-folder cleanup
  (WP-04, WP-10, [ADR 0005](adr/0005-cleanup-safety.md)), RemoveFoundFiles and UnusualAct.
- Tuning: apply and revert (WP-05).
- The plugin `CleanupProvider` and `ActionProvider`, and cleanup, tuning and rule packs.
- The CLI's `end`, `cap` and `undo`.

## Known gaps in the foundation

What the code does today, read from the code on 2026-10-22. Each is a small task for whoever owns the file.

| # | Gap | Where | Fix |
| --- | --- | --- | --- |
| G1 | Dropped with cleanup (ADR 0021). Was done in WP-04 (`Os::Linux`, XDG roots, Linux categories). Was: the cleanup category `Os` enum has `Windows`, `Macos`, `Any` but no `Linux`, so Linux has no categories or roots | `crates/dyno-core/src/cleanup/catalog.rs`, `crates/dyno-agent/src/cleanup/roots.rs` | Add `Os::Linux`, XDG roots (`~/.cache`, `~/.local/share/Trash`, `~/.cargo`, …), catalogue entries and tests |
| G2 | `StartupLocation` has Windows and macOS variants only; the Linux startup adapter (XDG autostart, systemd user units) has nowhere to put its entries | `crates/dyno-core/src/startup.rs`, `crates/dyno-platform/src/linux/startup.rs` | Add `XdgAutostart`, `SystemdUserUnit` (and `needs_admin` for system units), map them, keep the wire contract in step |
| G4 | The history `crashes` list is always empty and the health report gets `crashes_7d: 0`, so the crashes chart and the "crashing apps" health reason have no data | `crates/dyno-agent/src/handlers.rs` (`history`, `health`) | Done in WP-06: `history.crashes` is per local day from stored AppCrash issues, `crashes_7d` and the weekly report count them |
| G5 | Plugins are wired minimally: the agent starts the host on first `Request::Plugins` and lists plugins; sensor contributions are not yet connected to the sampler (plugins are sensors only, ADR 0021) | `crates/dyno-agent/src/handlers.rs`, `sampler.rs`, `crates/dyno-plugin-host` | Connect sensor contributions through guarded adapters (ADR 0007) and remove the cleanup and action providers from the host |
| G6 | Superseded: the helper is not in 1.0 (planned for 1.1, ADR 0022) | `crates/dyno-helper` | superseded |
| G7 | `DiskUsage` and the weekly report are built (WP-04, WP-08); no request answers `Unsupported` for being unbuilt any more | `crates/dyno-agent/src/handlers.rs` | done |
| G8 | Windows wires sensors (PDH) and user activity only. Process details use `NoDetails`; file locks (Restart Manager), startup entries and facts are the in-memory fakes | `crates/dyno-platform/src/windows/mod.rs` | Windows details probe (command line, modules, connections, signature), Restart Manager locks, registry / Startup folder / scheduled-task startup, facts (disk health, updates, power plan); verify on the Windows CI runner |
| G9 | The doc comments on `Overhead::agent_cpu` and `Load::own_cpu` say "percent of the machine", but the sampler multiplies by the core count: the value is a share of one core (ADR 0002 section 4) | `crates/dyno-core/src/protocol.rs`, `sampling.rs` | Fix the comments (and the UI label) or change the unit; keep the budget comparison consistent |
| G10 | Done for the kept screens. Was: placeholders for CPU taming, Cleanup and Health screens; taming and cleanup are dropped (ADR 0021) | | done |
| G11 | Done in WP-14 (`dyno_platform::toolpath`, ADR 0020). Was: the login-shell `PATH` resolver did not exist; nothing started user tools | `crates/dyno-platform/src/toolpath.rs` | done: bounded, timed out, cached, with a validated per-tool override; WP-15 reuses it |
| G12 | On Windows nothing restarts a crashed agent until the app or the next logon does | `crates/dyno-agent/src/service` | Optional: a Task Scheduler "restart on failure" entry |
| G13 | Dyno's overhead was measured once, under a high load average, on one machine; there is no CI artifact (`dyno-agent bench`) per OS | `docs/architecture.md`, `.gitea/workflows/dyno-release.yml` and `apps/dyno/paddock.ci.json` (repo root) | Run `bench` on release builds on a quiet runner per OS, record load next to the numbers, fail on a regression |
| G15 | CI could not be run when it was written: the workflow, `scripts/check-bundle.sh`, and `scripts/check-capabilities.mjs` are validated as YAML and with shell syntax checks and unit tests only | `.gitea/workflows/dyno-release.yml` and `apps/dyno/paddock.ci.json` (repo root), `scripts/` | Run on the three runners and fix what the first runs show |

## Dyno's own overhead

Measured with `dyno-agent bench 25 none|summary|processes` on the 14-core M4 Mac mini, **while the load average was about
55 to 85**: about 2.0 % (idle, nobody watching), 2.7 % (a summary subscriber) and 3.2 % (a processes subscriber) of **one**
core, and about 24 MB resident (0.15 to 0.25 % of the whole machine). Because the machine was busy, treat these as upper
bounds. The target is at most about 1 % of the machine. See [architecture.md](architecture.md#dynos-own-overhead) and G13.

## WP-01 Privileged helper (planned for 1.1)

Not in Dyno 1.0 (decided 2026-10-28; ADR 0022 holds the design, the per-OS table of what 1.0 cannot show, and the 1.1 plan).

- **1.0 instead.** Everything runs with the user's rights; no system service is installed. A figure the OS withholds is *Unavailable*
  with a plain reason, never 0. Other accounts' processes are one "Other users" row.
- **1.1.** The read-only helper installed with the app from a protected folder, **start as administrator at login**, all
  processes and administrator-only counters on all three OS, and the helper installing Dyno's own signed updates.
  Before it ships, its launchd plist (`crates/dyno-helper/src/install.rs`, the `launchd_plist` text) changes from
  `ProcessType Background` to `Standard`: the helper answers the agent's requests, and launchd runs a `Background` job as
  DARWIN_BG with every thread clamped to the `BACKGROUND` QoS band (rule 52; lessons 2026-10-30).
- **Acceptance (1.0).** No administrator prompt at install or start; nothing runs elevated; unreadable figures say why.

## WP-02 CPU taming screen and profiles

Dropped ([ADR 0021](adr/0021-monitoring-only.md)); see [Dropped](#dropped).

## WP-03 Health score and startup items (view only)

- **Scope.** The Health page: the health score with its reasons, and the startup list (impact, filter for OS and signed
  entries). No fix buttons and no enabling or disabling ([ADR 0021](adr/0021-monitoring-only.md)).
- **Acceptance.** Impact is measured, not guessed; the page works with no data (a fresh install).
- **Depends on.** G2 (Linux), G8 (Windows).
- **Touches.** `app/src/app/features/health/**`, `state/health.store.ts`, `crates/dyno-core/src/health.rs`, `startup.rs`,
  `crates/dyno-platform/src/{windows,macos,linux}/startup.rs`, `docs/screenshots/health-*.png`.

## WP-04 Cleanup with quarantine and undo

Dropped ([ADR 0021](adr/0021-monitoring-only.md)); see [Dropped](#dropped). The disk-space map (`DiskUsage`: bounded in depth, entries and time) stays, with its screen, the Disk map page, built.

## WP-05 Tuning catalogue

Dropped ([ADR 0021](adr/0021-monitoring-only.md)); see [Dropped](#dropped).

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
  entry or scheduled task). Each flag explains itself and offers "ignore" or "search the web" (WP-09).
- **Acceptance.** No verdicts ("malware"): wording is "unusual because …"; a pure scoring function with tests for each flag;
  a fresh install learns a baseline silently for a configurable period instead of flagging everything; flags never trigger an
  action by themselves.
- **Status (built).** `dyno_core::unusual` (pure `assess` with a test per flag; `Baseline::observe` with silent learning,
  default 7 days, 0 to 60 in Settings, the very first look always silent), agent `unusual` centre (look every 30 s, signatures
  of programs never seen checked 3 at a time off the sampler, one activity line when a program becomes unusual), tables
  `baseline` and `baseline_startup`, "New and unusual" view on the Issues page. Actions:
  Hide and Restore (Dyno's own list); none acts on the program. Gaps: "search the web" waits for WP-09; "sudden network" is approximated as many
  open connections (a rate needs per-process traffic, WP-37 has it on macOS and Linux only); signatures need process details,
  so Windows flags no "unsigned" until G8.
- **Depends on.** G8 (Windows details and signatures), WP-06 for the shared page.
- **Touches.** new `crates/dyno-core/src/unusual.rs`, `store.rs`, handlers, `app/src/app/features/issues/**` or `processes/**`.

## WP-08 Alerts and the weekly report

- **Scope.** The Reports page: alert history (low disk, overheating, crash loops, throttling), notification delivery (OS
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

Dropped ([ADR 0021](adr/0021-monitoring-only.md)); see [Dropped](#dropped).

## WP-11 Benchmarks on click

- **Status.** Built (ADR 0015). `crates/dyno-bench` (kernels and the child process), `dyno_core::bench` (types, plan validation,
  comparison, bounded history), `dyno-agent/src/bench.rs` (one run at a time, per-kind timeout, scratch folder removed by the
  agent), requests `benchmarks`, `startBenchmark`, `cancelBenchmark`, `deleteBenchmark`, `compareBenchmarks`; UI page
  `/benchmarks`; screenshots `benchmarks-*.png`.
- **Open.** GPU compute (reported unavailable), cancelling a kind
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
  rates, Windows traffic (IPv6 and ETW as well), UDP bytes; per-process traffic rates and sparklines in group rows; a row context menu; the user column in the detail panel;
  the tab in process details; resolving other users' sockets (1.1, with the helper); rules that alert on a new non-loopback
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
| Large and duplicate file finder | Scan a chosen folder for big or duplicate files; view only, nothing is removed | |
| Maintenance windows | Times when Dyno's heavier background work may run | Setting plus scheduler |

## WP-14 Backup health checks

- **Scope.** Check that backups ran and succeeded: Time Machine (`tmutil`), Windows Backup and File History, restic, Borg and
  Duplicati (via their logs or CLIs, declared as tier-2 detector packs). Dyno never runs or manages backups.
- **Acceptance.** Tools are run with timeouts and the user's login-shell `PATH` (G11); "no backup tool found" is a normal state;
  a stale backup raises an alert with a plain sentence.
- **Depends on.** G11, WP-15, WP-08.
- **Touches.** `crates/dyno-core/src/backup.rs` (new), pack detectors, adapter command runners, UI card.

## WP-15 Declarative TOML packs (Later, after 1.0)

- **Scope.** Tier 2 of ADR 0007, read-only data only: backup detectors and known helpers (no cleanup, tuning or rule packs,
  [ADR 0021](adr/0021-monitoring-only.md)). A pack is parsed once at the edge (`deny_unknown_fields`, size and count limits),
  validated; invalid packs are skipped with a message naming the file and line; a format version gates loading. Packs ship in
  the app and may be dropped into the user's `packs/` folder.
- **Acceptance.** A corrupt pack never stops the app; the default profile file documents every key (the schema is its own
  documentation, like Debrief's).
- **Depends on.** G11.
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
  need root, so in 1.0 the page says so; the helper reads them in 1.1.
- **Acceptance.** Empty slots are drawn empty; an unreadable source says why; an unreadable Linux read says it needs root and the app works without it.
- **Depends on.** Nothing in 1.0 (the Linux read waits for the helper in 1.1). **Touches.** `dyno-platform/**`, a facts type in the core, a memory detail page.

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
- **Built (2026-10-23, [ADR 0018](adr/0018-throttling-lifespan-and-baselines.md)).** `throttle.rs` (detector, episode tracker),
  `dyno-platform/src/hardware.rs` (`pmset -g therm`, Linux sysfs counters), `agent/src/hardware.rs`, the Throttling page, the cause in
  Why slow, alarms at medium confidence or better. **Left:** GPU throttling (only the CPU is judged; no GPU clock source on any OS
  yet), Windows reports no throttle reason (inferred, CI-only), and the episode in progress is lost if the agent crashes.

## WP-24 Disk and battery life forecasts

- **Scope.** SSD wear (SMART or NVMe data, total bytes written against rated TBW) as a remaining-years estimate; battery
  capacity trend and cycle count as "replace in about N months".
- **Acceptance.** Estimates show a range and the data they stand on; too little history says so; no estimate without data.
- **Touches.** `dyno-core/src/forecast.rs`, facts adapters, history of capacity and wear.
- **Built (2026-10-23).** `lifespan.rs` (a robust Theil-Sen range, replacement and clock-change handling), readers behind
  `DiskWearSource` / `BatteryCapacitySource` (smartctl, ioreg, sysfs, powercfg), `hardware_readings` in the store, the Lifespan page,
  Health reasons, the weekly report and alarms. **Left:** SSD wear without smartmontools needs the helper (WP-01, 1.1); the rated TBW is
  used only when a reader supplies it (no vendor table); the Windows battery and smartctl readers run only in CI.

## WP-25 "Is this normal?" baselines

- **Scope.** Learn per metric and app a baseline by time of day and day of week from history; highlight values outside it on
  the dashboard and feed alarms.
- **Acceptance.** A pure statistics function with tests; a new install learns silently for a configurable period; highlights
  carry the words ("higher than usual for Tuesday mornings"), not colour alone.
- **Touches.** `dyno-core/src/stats.rs` and `anomaly.rs`, history queries, dashboard.
- **Built (2026-10-23).** `baseline.rs` (learning, fallbacks from weekday-and-hour to weekday-or-weekend to hour, judgement in words,
  persistence for alarms), `agent/src/baselines.rs` (learned from the quarter-hour tier and the minute rows, cached for 30 min), the
  dashboard card and Settings. **Left:** per-app baselines use only the minute rows (14 days by default) and so carry lower
  confidence; no baseline chart (the card states the usual value and range in words).

## WP-26 Alarms view and custom alerts

- **Scope.** ADR 0013 in full: the pure state machine, custom alerts on any metric evaluated by the agent, Active, History and Statistics views, acknowledge, shelve, mute, notes, Open in Debrief, tray and
  dashboard badge, native notifications with Acknowledge and Snooze, quiet hours with a critical bypass, bounded SQLite
  retention.
- **Acceptance.** Every (state, event) pair is tested; repeats de-duplicate; shelving expires correctly across a reboot; active
  alarms are never pruned; notifications never contain file contents.
- **Touches.** `dyno-core/src/alarms.rs` (in progress), `alerts.rs` (superseded), `store.rs`, handlers, `features/alarms/**`.

## WP-27 `dyno` CLI

- **Built.** ADR 0014: `dyno status`, `why-slow`, `top`, `alarms`, `snapshot` and `counters`, over the same protocol and discovery
  file; read-only (the former `end`, `cap` and `undo` are dropped, ADR 0021). Refuses a wrong protocol version with a clear message;
  output is stable JSON. The binary is a sidecar of every installer (`bundle.externalBin`, checked by `scripts/check-bundle.sh`).
- **Touches.** `crates/dyno-cli`, `app/src-tauri/scripts/sidecar.mjs`, docs/cli.md.

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
- **WP-36 Taming acceptance rules.** Dropped with taming (ADR 0021).

**Sampling lesson (applied):** polling the macOS volume-space API often can stall Finder (an iStat Menus experience). The
common sensors now refresh free space at most once a minute; any new feature that touches volume space, the network stack
or other shared OS services must poll rarely or use change notifications.

## WP-30 Suite tie-ins

- **Scope.** The installed-apps inventory feeds Spotter's "watch for updates"; issues and alarms offer Open in Debrief;
  Pitcrew shows Dyno's machine health when both run; Pitwall's Explain this (WP-17). All through ADR 0014's protocol, on the
  user's click or opt-in.
- **Acceptance.** Each integration declares what it reads; none writes; each degrades to nothing when the other app is absent.

## WP-38 Customizable tray popup widgets

- **Scope.** The person chooses which small widgets the tray popup shows, and their order, from a list such as active alarms,
  disk space per volume, CPU and memory now, network now, backup health, last issue and pinned counters. In 1.0 (user, 2026-10-31: "In Dyno 1.0, delay the tag"); design in family ADR [0035](../../../docs/adr/0035-widget-layouts-are-shared.md).
- **Settings.** The choice persists in settings as a versioned list, with sensible defaults (alarms, disk space, CPU and memory);
  damaged or unknown entries fall back to the defaults with a note, as WP-21 does.
- **Data.** Values come from the engine through its existing operations; the UI formats them and adds no domain logic (ER-22 and
  the Paddock rule on where logic lives).
- **States.** Every widget has a designed empty, loading and error state, each with a short next step.
- **Accessibility.** The popup is keyboard-operable: a logical focus order follows the widget order, every widget has a label, and
  the chooser's reordering works from the keyboard.
- **Shared.**
  - The popover host, height fitting and closing already exist in `crates/paddock-window` (`popover.rs`, `tauri/popover.rs`,
    `tray.rs`) and `ui/projects/paddock-ui/src/lib/tray-popover.ts`; WP-38 uses them and adds nothing to them.
  - Dyno's widget and tray-item layout model (ADR 0012: `WidgetInstance` and `TrayItem` layouts as versioned settings data, a
    closed catalogue per app, bounded counts, presets with undo) is generalised into a shared crate (concept name
    `paddock-widgets`) plus shared `paddock-ui` widget-area and chooser components. Each app registers its catalogue.
  - Dyno moves onto it first and deletes its copy (`known-copies.toml`, mechanism `widget-layouts`, delete by `dyno-v1.0.0`).
- **Acceptance.** Choose, reorder and persist across restarts; the popup renders within the existing popover limits (size, placement
  and closing on blur, WP-22); tests cover each widget's empty, loading and error states.

## Later (after 1.0)
<a id="after-v1"></a>


Not part of 1.0.0; anything new that only watches goes here. Anything that would change the machine needs a new ADR first.

- **Multi-PC LAN view.** See several of your own PCs on one dashboard over the LAN, opt-in, read-only, over **TLS with a
  pre-shared key**. Needs its own ADR (discovery, key exchange, certificate handling) before any code; depends on WP-16 and
  WP-27.
- **Disk map, after 1.0** (decided 2026-10-28; 1.0 has volumes, the sunburst drill-down, Retry of the incomplete folders, Reveal
  and the Full Disk Access prompt): duplicate files, the big and old file lists, and the breakdown of hidden space (1.0 shows
  one figure, used minus counted).
- **Disk map on Windows, faster (open, after 1.0).** Each file is opened once (`CreateFileW`, then two queries) to get its volume
  serial, file index, link count and allocation size. Reading ids and allocation sizes for a whole folder from one folder handle
  (`GetFileInformationByHandleEx` with `FileIdExtdDirectoryInfo`) would cut that, but it carries no link count, so hard links
  would need another way to be found. Measure on a real Windows disk before choosing. Follow-ups: take the folder id and the
  allocation size from enumeration and open a handle only for files (for their link count); on Windows 11 24H2 and later,
  `GetFileInformationByName` reads those without opening the file at all.
- **1.0 manual check, disk map on Windows:** measure files per second on a real NTFS volume with Defender on (real-time
  protection scans every open), and compare with the figure on macOS before the release.
- **WP-15, 16, 17, 30 and 31** (packs, web API, Explain via Pitwall, suite tie-ins, session recording) are planned here, after 1.0.
- Further ideas land here as they come up.

## Known gaps in the jobs (2026-10-29)

- **Windows Event Log streaming.** `wevtutil` is read in one step (output parsed after it ends), so Cancel of a Windows log read
  takes effect when the tool has answered and the phase has no record count. macOS streams `log show` and kills it at once;
  the same for `wevtutil qe` is the next step (`IssueSource::read_controlled` and `ToolRunner::run_lines` are the seam).
- **Jobs at once** is read at start: resizing the pool while jobs run is not built.

## Screenshot checklist

`npm run screenshots` in `app/` writes `docs/screenshots/<screen>-<dark|light>.png` against the mock backend: pages from
`e2e/screenshots.spec.ts`, the dialogs (Settings, the web and hash searches) as their own windows from
`e2e/screenshots-docs-dialogs.spec.ts`. A row is done when both themes exist and show the built screen.

| Feature | Screenshot | State |
| --- | --- | --- |
| Dashboard (live tiles, diagnosis, health) | `dashboard-*.png`, `dashboard-full-*.png` | captured |
| Processes (grouped tree, details) | `processes-*.png` | captured |
| Settings (the Settings window) | `settings-*.png`, `settings-menu-bar-*.png` | captured |
| Tray popover on a desktop backdrop | `tray-*.png` | captured |
| Why slow | `why-slow-*.png` | captured |
| Health | `health-*.png` | captured |
| Health startup filter | `health-startup-filter-*.png` | not captured |
| Reports | `reports-*.png` | captured |
| Issues with statistics | `issues-*.png` | captured |
| Unusual processes | `unusual-*.png` | captured |
| Web and hash search (browser hand-offs) | `web-search-*.png`, `hash-search-*.png` | captured |
| Benchmarks | `benchmarks-*.png` | captured |
| Benchmarks, running and compare | `benchmarks-running-*.png`, `benchmarks-compare-*.png` | not captured |
| Network | `network-connections-*.png` | captured |
| Network, listening ports | `network-*.png` | not captured |
| Performance counters | `counters-*.png` | captured |
| Performance counters, search | `counters-search-*.png` | not captured |
| Backup health | `backup-*.png` | captured |
| Per-core CPU tile (total and per-core toggle, P/E grouping) | `cpu-per-core-tile-*.png` | not captured (WP-19) |
| CPU detail with per-core heatmap | `cpu-detail-*.png` | captured (WP-19) |
| Widget gallery and dashboard edit mode | `widgets-edit-*.png` | captured (WP-21) |
| Presets | `widgets-developer-*.png`, `widgets-battery-*.png`, `widgets-gaming-*.png`, `widgets-overview-*.png` | not captured (WP-21) |
| Customised menu bar (several tray items) | `settings-menu-bar-*.png` | captured (WP-21) |
| Memory slot drawings, Apple Silicon unified memory | `inventory-*.png` | captured (WP-20) |
| Memory slot drawings, DDR5, generic, laptop and soldered | `inventory-ddr5-*.png`, `inventory-generic-*.png`, `inventory-laptop-*.png`, `inventory-soldered-*.png` | not captured (WP-20) |
| Alarms: Active | `alarms-*.png` | captured (WP-26) |
| Alarms: History | `alarms-history-*.png` | captured (WP-26) |
| Alarms: Rules, with the built-in alert statistics | `alarms-rules-*.png` | captured (WP-26) |
| Diagnostics bundle preview | `diagnostics-*.png` | captured (WP-29) |
| Throttling and forecasts | `throttling-*.png`, `forecasts-*.png` | captured (WP-23, WP-25) |
| Throttling while hot, "Is this normal?" | `throttling-hot-*.png`, `baselines-*.png` | not captured (WP-24) |
| Extras: What changed, Boot, Apps, Files | `extras-changes-*.png`, `extras-boot-*.png`, `extras-apps-*.png`, `extras-files-*.png` | captured (WP-13) |
| Extras: Sleep, GPU, Inventory, Maintenance | `extras-sleep-*.png`, `extras-gpu-*.png`, `extras-inventory-*.png`, `extras-maintenance-*.png` | not captured (WP-13) |
| Flight recorder | `recorder-*.png` | captured |
| Disk map (chart and table) | `disk-map-*.png`, `disk-map-table-*.png` | captured |
| Command palette | `command-palette-*.png` | captured |
| Jobs (a running job with its phase and Cancel) | `jobs-*.png` | captured |
| Cleanup, CPU taming rules, tuning, watched folders | none | not in 1.0 (dropped, [ADR 0021](adr/0021-monitoring-only.md)) |

Follow-ups recorded here: the mock's live values still drift a little between a light and a dark shot (they derive from the real clock,
not only the faked one), and the engine should send each alarm rule's display name so the UI stops deriving it from the rule key
(`ruleLabel` in `alarms-model.ts`) and a custom rule shows the name its owner gave it.
